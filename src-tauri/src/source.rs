//! Finds Claude Code (Windows PATH or a WSL distro) and runs it with a timeout.

use crate::settings::SourcePref;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

pub const CLI_TIMEOUT: Duration = Duration::from_secs(60);
const PROBE_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    Windows,
    Wsl { distro: String },
    NotFound,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RunError {
    Timeout,
    Exit(i32, String),
    Spawn(String),
}

pub trait Runner {
    fn run(&self, program: &str, args: &[&str], timeout: Duration) -> Result<String, RunError>;
}

pub struct SystemRunner;

impl Runner for SystemRunner {
    fn run(&self, program: &str, args: &[&str], timeout: Duration) -> Result<String, RunError> {
        let mut cmd = Command::new(program);
        cmd.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = cmd.spawn().map_err(|e| RunError::Spawn(e.to_string()))?;
        #[cfg(windows)]
        let job = job::Job::around(&child);
        // Read both pipes on their own threads so a full pipe buffer cannot stall the child.
        let drain = |pipe: Option<Box<dyn Read + Send>>| {
            let (tx, rx) = mpsc::channel();
            thread::spawn(move || {
                let mut buf = Vec::new();
                if let Some(mut p) = pipe {
                    let _ = p.read_to_end(&mut buf);
                }
                let _ = tx.send(buf);
            });
            rx
        };
        let out = drain(child.stdout.take().map(|p| Box::new(p) as _));
        let err = drain(child.stderr.take().map(|p| Box::new(p) as _));

        let deadline = Instant::now() + timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break s,
                Ok(None) if Instant::now() >= deadline => {
                    // Killing wsl.exe may leave the Linux-side claude running; the WSL command
                    // has its own `timeout 55` for that.
                    #[cfg(windows)]
                    job.terminate();
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(RunError::Timeout);
                }
                Ok(None) => thread::sleep(Duration::from_millis(50)),
                Err(e) => return Err(RunError::Spawn(e.to_string())),
            }
        };
        // A descendant that inherited the pipe can keep it open after the child exits, so the
        // reads share the same deadline. Stdout missing in time is a timeout; stderr is optional.
        let left = |d: Instant| d.saturating_duration_since(Instant::now());
        let out = out.recv_timeout(left(deadline)).map_err(|_| RunError::Timeout)?;
        let err = err.recv_timeout(left(deadline)).unwrap_or_default();
        if status.success() {
            Ok(decode_output(&out))
        } else {
            Err(RunError::Exit(status.code().unwrap_or(-1), decode_output(&err)))
        }
    }
}

/// A Windows Job Object that kills every process the child started when it is closed or
/// terminated. Best effort: if any call fails the run goes on without it.
#[cfg(windows)]
mod job {
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::*;

    pub struct Job(Option<HANDLE>);

    impl Job {
        pub fn around(child: &Child) -> Job {
            let made = unsafe { Self::create(HANDLE(child.as_raw_handle())) };
            if let (Err(e), true) = (&made, cfg!(debug_assertions)) {
                eprintln!("job object unavailable: {e}");
            }
            Job(made.ok())
        }

        unsafe fn create(child: HANDLE) -> windows::core::Result<HANDLE> {
            let job = CreateJobObjectW(None, None)?;
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let set = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of_val(&info) as u32,
            )
            .and_then(|_| AssignProcessToJobObject(job, child));
            if let Err(e) = set {
                let _ = CloseHandle(job);
                return Err(e);
            }
            Ok(job)
        }

        pub fn terminate(&self) {
            if let Some(h) = self.0 {
                let _ = unsafe { TerminateJobObject(h, 1) };
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            if let Some(h) = self.0 {
                let _ = unsafe { CloseHandle(h) };
            }
        }
    }
}

/// `wsl.exe -l` prints UTF-16LE; everything else is UTF-8.
fn decode_output(bytes: &[u8]) -> String {
    let bom = bytes.starts_with(&[0xFF, 0xFE]);
    let nul_odd = bytes.len() >= 2
        && bytes.len() % 2 == 0
        && bytes.iter().skip(1).step_by(2).filter(|&&b| b == 0).count() * 2 > bytes.len() / 2;
    if bom || nul_odd {
        let body = if bom { &bytes[2..] } else { bytes };
        let units: Vec<u16> = body.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

fn lines(out: &str) -> impl Iterator<Item = &str> {
    out.lines().map(str::trim).filter(|l| !l.is_empty())
}

fn wsl_has_claude(runner: &dyn Runner, distro: &str) -> bool {
    runner
        // Probing a stopped distro boots it, so give it the full CLI timeout.
        .run("wsl.exe", &["-d", distro, "--", "bash", "-lc", "command -v claude"], CLI_TIMEOUT)
        .is_ok()
}

/// Looks for Claude. With `start_wsl` off it must not boot a stopped distro, so it only probes
/// distros that are already running; finding nothing then returns None (try again later),
/// never NotFound.
pub fn detect_with(runner: &dyn Runner, pref: &SourcePref, start_wsl: bool) -> Option<Source> {
    match pref {
        SourcePref::Windows => return Some(Source::Windows),
        SourcePref::Wsl { distro } => return Some(Source::Wsl { distro: distro.clone() }),
        SourcePref::Auto => {}
    }
    if runner.run("where.exe", &["claude"], PROBE_TIMEOUT).is_ok() {
        return Some(Source::Windows);
    }
    let list_args: &[&str] = if start_wsl { &["-l", "-q"] } else { &["-l", "--running", "-q"] };
    let found = runner
        .run("wsl.exe", list_args, PROBE_TIMEOUT)
        .ok()
        .and_then(|list| lines(&list).find(|d| wsl_has_claude(runner, d)).map(String::from));
    match found {
        Some(distro) => Some(Source::Wsl { distro }),
        None if start_wsl => Some(Source::NotFound),
        None => None,
    }
}

#[cfg(test)]
fn detect(runner: &dyn Runner, pref: &SourcePref) -> Source {
    detect_with(runner, pref, true).unwrap_or(Source::NotFound)
}

/// Program and arguments that print the usage JSON. `NotFound` has none to run.
pub fn usage_command(source: &Source) -> (String, Vec<String>) {
    let (program, args): (&str, Vec<&str>) = match source {
        // cmd /c launches both claude.exe and an npm claude.cmd shim.
        Source::Windows | Source::NotFound => (
            "cmd.exe",
            vec![
                "/d", "/c", "claude", "-p", "--settings", r#"{"disableAllHooks":true}"#,
                "--output-format", "json", "/usage",
            ],
        ),
        // The login shell is required: a plain `wsl -- claude` lacks ~/.local/bin on PATH.
        Source::Wsl { distro } => (
            "wsl.exe",
            vec![
                "-d", distro, "--", "bash", "-lc",
                r#"timeout 55 claude -p --settings '{"disableAllHooks":true}' --output-format json /usage"#,
            ],
        ),
    };
    (program.to_string(), args.into_iter().map(String::from).collect())
}

/// Installed WSL distros in `wsl -l` order (listing does not boot any of them).
pub fn list_distros(runner: &dyn Runner) -> Vec<String> {
    runner
        .run("wsl.exe", &["-l", "-q"], PROBE_TIMEOUT)
        .map(|out| lines(&out).map(String::from).collect())
        .unwrap_or_default()
}

pub fn wsl_running(runner: &dyn Runner, distro: &str) -> bool {
    runner
        .run("wsl.exe", &["-l", "--running", "-q"], PROBE_TIMEOUT)
        .map(|out| lines(&out).any(|l| l == distro))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    struct FakeRunner {
        map: HashMap<String, Result<String, RunError>>,
        calls: RefCell<Vec<String>>,
    }

    impl FakeRunner {
        fn new(entries: &[(&str, Result<String, RunError>)]) -> Self {
            FakeRunner {
                map: entries.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(),
                calls: RefCell::new(vec![]),
            }
        }
    }

    impl Runner for FakeRunner {
        fn run(&self, program: &str, args: &[&str], _t: Duration) -> Result<String, RunError> {
            let key = format!("{} {}", program, args.join(" "));
            self.calls.borrow_mut().push(key.clone());
            self.map.get(&key).cloned().unwrap_or(Err(RunError::Spawn("unknown".into())))
        }
    }

    fn fail() -> Result<String, RunError> {
        Err(RunError::Exit(1, String::new()))
    }

    #[test]
    fn prefers_windows_when_present() {
        let r = FakeRunner::new(&[("where.exe claude", Ok("C:\\x\\claude.exe\r\n".into()))]);
        assert_eq!(detect(&r, &SourcePref::Auto), Source::Windows);
    }

    #[test]
    fn falls_back_to_wsl_distro() {
        let r = FakeRunner::new(&[
            ("where.exe claude", fail()),
            ("wsl.exe -l -q", Ok("Ubuntu\r\n".into())),
            ("wsl.exe -d Ubuntu -- bash -lc command -v claude", Ok("/home/u/.local/bin/claude\n".into())),
        ]);
        assert_eq!(detect(&r, &SourcePref::Auto), Source::Wsl { distro: "Ubuntu".into() });
    }

    #[test]
    fn probes_distros_in_order_and_stops_at_first_hit() {
        let r = FakeRunner::new(&[
            ("where.exe claude", fail()),
            ("wsl.exe -l -q", Ok("docker-desktop\r\n\r\nUbuntu\r\nDebian\r\n".into())),
            ("wsl.exe -d docker-desktop -- bash -lc command -v claude", fail()),
            ("wsl.exe -d Ubuntu -- bash -lc command -v claude", Ok("/x/claude".into())),
            ("wsl.exe -d Debian -- bash -lc command -v claude", Ok("/x/claude".into())),
        ]);
        assert_eq!(detect(&r, &SourcePref::Auto), Source::Wsl { distro: "Ubuntu".into() });
        assert_eq!(r.calls.borrow().len(), 4);
    }

    #[test]
    fn not_found() {
        let r = FakeRunner::new(&[("where.exe claude", fail()), ("wsl.exe -l -q", fail())]);
        assert_eq!(detect(&r, &SourcePref::Auto), Source::NotFound);
        let r = FakeRunner::new(&[
            ("where.exe claude", fail()),
            ("wsl.exe -l -q", Ok("Ubuntu\r\n".into())),
            ("wsl.exe -d Ubuntu -- bash -lc command -v claude", fail()),
        ]);
        assert_eq!(detect(&r, &SourcePref::Auto), Source::NotFound);
    }

    #[test]
    fn pref_overrides_auto() {
        let r = FakeRunner::new(&[]);
        let pref = SourcePref::Wsl { distro: "Debian".into() };
        assert_eq!(detect(&r, &pref), Source::Wsl { distro: "Debian".into() });
        assert_eq!(detect(&r, &SourcePref::Windows), Source::Windows);
        assert!(r.calls.borrow().is_empty());
    }

    #[test]
    fn wsl_command_shape() {
        let (p, a) = usage_command(&Source::Wsl { distro: "Ubuntu".into() });
        assert_eq!(p, "wsl.exe");
        assert_eq!(
            a,
            [
                "-d", "Ubuntu", "--", "bash", "-lc",
                "timeout 55 claude -p --settings '{\"disableAllHooks\":true}' --output-format json /usage"
            ]
        );
    }

    #[test]
    fn windows_command_shape() {
        let (p, a) = usage_command(&Source::Windows);
        assert_eq!(p, "cmd.exe");
        assert_eq!(
            a,
            ["/d", "/c", "claude", "-p", "--settings", "{\"disableAllHooks\":true}", "--output-format", "json", "/usage"]
        );
    }

    #[test]
    fn wsl_running_parses_list() {
        let r = FakeRunner::new(&[("wsl.exe -l --running -q", Ok("Ubuntu\r\n".into()))]);
        assert!(wsl_running(&r, "Ubuntu"));
        assert!(!wsl_running(&r, "Debian"));
        let r = FakeRunner::new(&[("wsl.exe -l --running -q", Ok(String::new()))]);
        assert!(!wsl_running(&r, "Ubuntu"));
        let r = FakeRunner::new(&[("wsl.exe -l --running -q", fail())]);
        assert!(!wsl_running(&r, "Ubuntu"));
    }

    #[test]
    fn list_distros_skips_blanks_and_survives_failure() {
        let r = FakeRunner::new(&[("wsl.exe -l -q", Ok("Ubuntu\r\n\r\nDebian\r\n".into()))]);
        assert_eq!(list_distros(&r), ["Ubuntu", "Debian"]);
        let r = FakeRunner::new(&[("wsl.exe -l -q", fail())]);
        assert!(list_distros(&r).is_empty());
    }

    #[test]
    fn decodes_utf16_wsl_list() {
        let utf16: Vec<u8> = "Ubuntu\r\n".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        assert_eq!(decode_output(&utf16), "Ubuntu\r\n");
        let mut bom = vec![0xFF, 0xFE];
        bom.extend(&utf16);
        assert_eq!(decode_output(&bom), "Ubuntu\r\n");
        assert_eq!(decode_output(b"plain {\"a\":1}"), "plain {\"a\":1}");
    }

    #[cfg(windows)]
    #[test]
    fn runner_timeout_is_error() {
        let start = std::time::Instant::now();
        let r = SystemRunner.run(
            "powershell",
            &["-NoProfile", "-Command", "Start-Sleep 5"],
            Duration::from_secs(1),
        );
        assert_eq!(r, Err(RunError::Timeout));
        assert!(start.elapsed() < Duration::from_millis(2500));
    }

    #[cfg(windows)]
    #[test]
    fn exited_child_with_held_pipe_times_out() {
        // The parent exits at once; its child inherits the stdout pipe and sleeps.
        let start = std::time::Instant::now();
        let r = SystemRunner.run(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "Start-Process -NoNewWindow powershell -ArgumentList '-NoProfile','-Command','Start-Sleep 5'; exit 0",
            ],
            Duration::from_secs(1),
        );
        assert_eq!(r, Err(RunError::Timeout));
        assert!(start.elapsed() < Duration::from_millis(2500));
    }

    #[cfg(windows)]
    #[test]
    fn timeout_kills_grandchildren() {
        // The parent starts a grandchild that would create a marker file after 3 s.
        let marker = std::env::temp_dir().join(format!("notch-job-{}", std::process::id()));
        let _ = std::fs::remove_file(&marker);
        let script = format!(
            "Start-Process -NoNewWindow powershell -ArgumentList '-NoProfile','-Command','Start-Sleep 3; New-Item -ItemType File -Path \"{}\"'; Start-Sleep 10",
            marker.display()
        );
        let r = SystemRunner.run("powershell", &["-NoProfile", "-Command", &script], Duration::from_secs(1));
        assert_eq!(r, Err(RunError::Timeout));
        std::thread::sleep(Duration::from_secs(4));
        assert!(!marker.exists(), "grandchild survived the timeout");
    }

    #[test]
    fn start_wsl_off_probes_only_running_distros() {
        let r = FakeRunner::new(&[
            ("where.exe claude", fail()),
            ("wsl.exe -l --running -q", Ok("Ubuntu\r\n".into())),
            ("wsl.exe -d Ubuntu -- bash -lc command -v claude", Ok("/x/claude\n".into())),
        ]);
        assert_eq!(
            detect_with(&r, &SourcePref::Auto, false),
            Some(Source::Wsl { distro: "Ubuntu".into() })
        );
        let r = FakeRunner::new(&[("where.exe claude", fail()), ("wsl.exe -l --running -q", Ok(String::new()))]);
        assert_eq!(detect_with(&r, &SourcePref::Auto, false), None);
        assert!(r.calls.borrow().iter().all(|c| !c.contains(" -d ") && !c.contains("-l -q")));
    }
}
