//! Prayer times from JAKIM's e-solat month data (prayer spec 5.1-5.3): parsing, the next and
//! previous prayer, and which alert is due. Pure, so it is tested without Tauri or the network.

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Asia::Kuala_Lumpur;
use crate::source::Runner;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prayer {
    Subuh,
    Zohor,
    Asar,
    Maghrib,
    Isyak,
}

pub const PRAYERS: [Prayer; 5] = [Prayer::Subuh, Prayer::Zohor, Prayer::Asar, Prayer::Maghrib, Prayer::Isyak];

impl Prayer {
    pub fn name(self) -> &'static str {
        match self {
            Prayer::Subuh => "Subuh",
            Prayer::Zohor => "Zohor",
            Prayer::Asar => "Asar",
            Prayer::Maghrib => "Maghrib",
            Prayer::Isyak => "Isyak",
        }
    }
}

/// One day of the five prayer times (UTC instants), dated in Malaysia time.
#[derive(Debug, Clone, PartialEq)]
pub struct Day {
    pub date: NaiveDate,
    pub times: [DateTime<Utc>; 5],
}

/// Which alerts already fired, as `YYYY-MM-DD-<Prayer>-<time|remind>` keys (saved across restarts).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Fired {
    keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Alert {
    /// It is time to pray.
    Time(Prayer, DateTime<Utc>),
    /// `minutes` before the prayer.
    Remind(Prayer, DateTime<Utc>, u32),
}

/// An alert older than this (e.g. the PC slept through it) is skipped, never shown late.
const ALERT_WINDOW: Duration = Duration::minutes(2);

/// JAKIM's month (`status` "OK!", `prayerTime` list of days). A day with a missing or broken
/// time is skipped; None when nothing usable is left.
pub fn parse_month(json: &str) -> Option<Vec<Day>> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    if v.get("status")?.as_str()? != "OK!" {
        return None;
    }
    let mut days: Vec<Day> = v
        .get("prayerTime")?
        .as_array()?
        .iter()
        .filter_map(|d| {
            let date = parse_date(d.get("date")?.as_str()?)?;
            let at = |key: &str| {
                let t = NaiveTime::parse_from_str(d.get(key)?.as_str()?, "%H:%M:%S").ok()?;
                Kuala_Lumpur.from_local_datetime(&date.and_time(t)).single().map(|x| x.with_timezone(&Utc))
            };
            Some(Day { date, times: [at("fajr")?, at("dhuhr")?, at("asr")?, at("maghrib")?, at("isha")?] })
        })
        .collect();
    days.sort_by_key(|d| d.date);
    (!days.is_empty()).then_some(days)
}

/// "01-Dis-2026": e-solat uses Malay month names (Mac, Mei, Ogos, Okt, Dis); English ones are
/// accepted too.
fn parse_date(s: &str) -> Option<NaiveDate> {
    let mut parts = s.split('-');
    let (day, month, year) = (parts.next()?.parse().ok()?, parts.next()?, parts.next()?.parse().ok()?);
    const MONTHS: [&[&str]; 12] = [&["jan"], &["feb"], &["mac", "mar"], &["apr"], &["mei", "may"], &["jun"], &["jul"],
        &["ogos", "ogo", "aug"], &["sep"], &["okt", "oct"], &["nov"], &["dis", "dec"]];
    let m = MONTHS.iter().position(|names| names.contains(&month.to_ascii_lowercase().as_str()))?;
    NaiveDate::from_ymd_opt(year, m as u32 + 1, day)
}

/// This month and next month (Malaysia time), whichever has no days cached. Next month is
/// fetched ahead so tomorrow's Subuh is known on the last evening of a month.
pub fn missing_months(days: &[Day], now: DateTime<Utc>) -> Vec<(i32, u32)> {
    let today = now.with_timezone(&Kuala_Lumpur).date_naive();
    let next = today.checked_add_months(chrono::Months::new(1)).unwrap_or(today);
    [today, next]
        .iter()
        .map(|d| (d.year(), d.month()))
        .filter(|&(y, m)| !days.iter().any(|d| d.date.year() == y && d.date.month() == m))
        .collect()
}

/// Where a zone's month is kept: `prayer-<ZONE>-<YYYY-MM>.json` in the app's folder.
pub fn cache_path(dir: &Path, zone: &str, month: (i32, u32)) -> PathBuf {
    dir.join(format!("prayer-{zone}-{}-{:02}.json", month.0, month.1))
}

/// Downloads one month with Windows' curl; the body only when it parses (prayer spec 5.1).
/// `period=duration` takes any month, so next month can be fetched before it starts.
pub fn fetch_month(runner: &dyn Runner, zone: &str, (year, month): (i32, u32)) -> Option<String> {
    let first = NaiveDate::from_ymd_opt(year, month, 1)?;
    let last = first.checked_add_months(chrono::Months::new(1))?.pred_opt()?;
    let range = format!("datestart={first}&dateend={last}");
    let url = format!("https://www.e-solat.gov.my/index.php?r=esolatApi/TakwimSolat&period=duration&zone={zone}");
    let body = runner
        .run("curl.exe", &["-s", "-f", "-m", "30", "-A", "ClaudeNotch", "-d", &range, &url], std::time::Duration::from_secs(35))
        .ok()?;
    parse_month(&body).map(|_| body)
}

/// This month's and next month's cached days (whatever is on disk), in order.
pub fn load_days(dir: &Path, zone: &str, now: DateTime<Utc>) -> Vec<Day> {
    let today = now.with_timezone(&Kuala_Lumpur).date_naive();
    let next = today.checked_add_months(chrono::Months::new(1)).unwrap_or(today);
    [today, next]
        .iter()
        .filter_map(|d| std::fs::read_to_string(cache_path(dir, zone, (d.year(), d.month()))).ok())
        .filter_map(|text| parse_month(&text))
        .flatten()
        .collect()
}

/// Every prayer moment in order.
fn moments(days: &[Day]) -> impl Iterator<Item = (NaiveDate, Prayer, DateTime<Utc>)> + '_ {
    days.iter().flat_map(|d| PRAYERS.iter().zip(d.times).map(move |(p, t)| (d.date, *p, t)))
}

/// The first prayer after `now` (None past the last cached day).
pub fn next(days: &[Day], now: DateTime<Utc>) -> Option<(Prayer, DateTime<Utc>)> {
    moments(days).find(|(_, _, t)| *t > now).map(|(_, p, t)| (p, t))
}

/// The latest prayer at or before `now`.
pub fn previous(days: &[Day], now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    moments(days).filter(|(_, _, t)| *t <= now).last().map(|(_, _, t)| t)
}

/// Today's times, by the date in Malaysia.
pub fn today(days: &[Day], now: DateTime<Utc>) -> Option<&Day> {
    let date = now.with_timezone(&Kuala_Lumpur).date_naive();
    days.iter().find(|d| d.date == date)
}

/// Alerts whose moment has come within the last two minutes and that have not fired yet; marks
/// them fired. Keys older than three days are dropped.
pub fn due(days: &[Day], now: DateTime<Utc>, remind_min: u32, fired: &mut Fired) -> Vec<Alert> {
    let mut out = Vec::new();
    for (date, p, t) in moments(days) {
        let mut check = |moment: DateTime<Utc>, kind: &str, alert: Alert| {
            let key = format!("{date}-{}-{kind}", p.name());
            if now >= moment && now - moment < ALERT_WINDOW && !fired.keys.contains(&key) {
                fired.keys.push(key);
                out.push(alert);
            }
        };
        if remind_min > 0 {
            check(t - Duration::minutes(i64::from(remind_min)), "remind", Alert::Remind(p, t, remind_min));
        }
        check(t, "time", Alert::Time(p, t));
    }
    let oldest = now.with_timezone(&Kuala_Lumpur).date_naive() - Duration::days(2);
    fired.keys.retain(|k| NaiveDate::parse_from_str(&k[..10.min(k.len())], "%Y-%m-%d").is_ok_and(|d| d >= oldest));
    out
}

/// The next prayer or reminder moment after `now`: when the prayer thread should wake.
pub fn next_wake(days: &[Day], now: DateTime<Utc>, remind_min: u32) -> Option<DateTime<Utc>> {
    moments(days)
        .flat_map(|(_, _, t)| {
            let r = (remind_min > 0).then(|| t - Duration::minutes(i64::from(remind_min)));
            [r, Some(t)]
        })
        .flatten()
        .filter(|m| *m > now)
        .min()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/jakim-WLY01.json");

    /// Malaysia local time on a November 2026 day.
    fn my(day: u32, h: u32, m: u32) -> DateTime<Utc> {
        Kuala_Lumpur.with_ymd_and_hms(2026, 11, day, h, m, 0).unwrap().with_timezone(&Utc)
    }

    fn days() -> Vec<Day> {
        parse_month(FIXTURE).expect("fixture parses")
    }

    #[test]
    fn parses_jakim_month() {
        let d = days();
        assert_eq!(d.len(), 30);
        let nov4 = d.iter().find(|x| x.date == NaiveDate::from_ymd_opt(2026, 11, 4).unwrap()).unwrap();
        assert_eq!(nov4.times[0], my(4, 5, 48)); // Subuh
        assert_eq!(nov4.times[1], my(4, 13, 0)); // Zohor
        assert_eq!(nov4.times[2], my(4, 16, 20)); // Asar
        assert_eq!(nov4.times[3], my(4, 18, 58)); // Maghrib
        assert_eq!(nov4.times[4], my(4, 20, 9)); // Isyak
        assert_eq!(Prayer::Maghrib.name(), "Maghrib");
    }

    #[test]
    fn bad_month_json_is_none() {
        assert!(parse_month("not json").is_none());
        assert!(parse_month(r#"{"status":"NO_RECORD!","prayerTime":[]}"#).is_none());
        assert!(parse_month(r#"{"status":"OK!"}"#).is_none());
        assert!(parse_month(r#"{"status":"OK!","prayerTime":[]}"#).is_none());
        // A day with a broken time is skipped, the rest still parse.
        let one_bad = r#"{"status":"OK!","prayerTime":[
            {"date":"04-Nov-2026","fajr":"05:48:00","dhuhr":"13:00:00","asr":"16:20:00","maghrib":"18:58:00","isha":"20:09:00"},
            {"date":"05-Nov-2026","fajr":"x","dhuhr":"13:00:00","asr":"16:20:00","maghrib":"18:58:00","isha":"20:09:00"}]}"#;
        assert_eq!(parse_month(one_bad).unwrap().len(), 1);
    }

    #[test]
    fn next_and_previous() {
        let d = days();
        assert_eq!(next(&d, my(4, 18, 0)), Some((Prayer::Maghrib, my(4, 18, 58))));
        assert_eq!(previous(&d, my(4, 18, 0)), Some(my(4, 16, 20)));
        assert_eq!(today(&d, my(4, 23, 59)).unwrap().date, NaiveDate::from_ymd_opt(2026, 11, 4).unwrap());
    }

    #[test]
    fn next_after_isyak_is_tomorrow_subuh() {
        let d = days();
        let (p, t) = next(&d, my(4, 21, 0)).unwrap();
        assert_eq!(p, Prayer::Subuh);
        assert_eq!(t.with_timezone(&Kuala_Lumpur).date_naive(), NaiveDate::from_ymd_opt(2026, 11, 5).unwrap());
    }

    #[test]
    fn month_end_without_next_month_is_none() {
        assert_eq!(next(&days(), my(30, 21, 0)), None);
    }

    /// e-solat writes dates with Malay month names: Mac, Mei, Ogos, Okt, Dis.
    #[test]
    fn parses_malay_month_names() {
        let day = |date: &str| format!(r#"{{"date":"{date}","fajr":"05:52:00","dhuhr":"13:05:00","asr":"16:27:00","maghrib":"19:02:00","isha":"20:16:00"}}"#);
        let json = format!(r#"{{"status":"OK!","prayerTime":[{}]}}"#,
            ["01-Mac-2026", "01-Mei-2026", "01-Ogos-2026", "01-Okt-2026", "01-Dis-2026", "01-Jan-2027"].map(day).join(","));
        let dates: Vec<_> = parse_month(&json).unwrap().iter().map(|d| (d.date.month(), d.date.year())).collect();
        assert_eq!(dates, vec![(3, 2026), (5, 2026), (8, 2026), (10, 2026), (12, 2026), (1, 2027)]);
    }

    #[test]
    fn month_end_with_next_month_is_tomorrow_subuh() {
        let mut d = days();
        d.extend(parse_month(r#"{"status":"OK!","prayerTime":[{"date":"01-Dis-2026","fajr":"05:52:00","dhuhr":"13:05:00","asr":"16:27:00","maghrib":"19:02:00","isha":"20:16:00"}]}"#).unwrap());
        let dec1_subuh = Kuala_Lumpur.with_ymd_and_hms(2026, 12, 1, 5, 52, 0).unwrap().with_timezone(&Utc);
        assert_eq!(next(&d, my(30, 21, 0)), Some((Prayer::Subuh, dec1_subuh)));
    }

    #[test]
    fn missing_months_are_this_and_next() {
        let d = days();
        assert_eq!(missing_months(&[], my(4, 12, 0)), vec![(2026, 11), (2026, 12)]);
        assert_eq!(missing_months(&d, my(4, 12, 0)), vec![(2026, 12)]);
        let mut both = d.clone();
        both.extend(parse_month(r#"{"status":"OK!","prayerTime":[{"date":"01-Dis-2026","fajr":"05:52:00","dhuhr":"13:05:00","asr":"16:27:00","maghrib":"19:02:00","isha":"20:16:00"}]}"#).unwrap());
        assert!(missing_months(&both, my(30, 21, 0)).is_empty());
    }

    #[test]
    fn due_fires_once() {
        let d = days();
        let mut f = Fired::default();
        assert_eq!(due(&d, my(4, 18, 58), 0, &mut f), vec![Alert::Time(Prayer::Maghrib, my(4, 18, 58))]);
        assert!(due(&d, my(4, 18, 59), 0, &mut f).is_empty());
        assert!(due(&d, my(4, 18, 30), 0, &mut f).is_empty());
    }

    #[test]
    fn reminder_fires_n_minutes_before() {
        let d = days();
        let mut f = Fired::default();
        assert_eq!(due(&d, my(4, 18, 48), 10, &mut f), vec![Alert::Remind(Prayer::Maghrib, my(4, 18, 58), 10)]);
        let mut g = Fired::default();
        assert!(due(&d, my(4, 18, 48), 0, &mut g).is_empty());
    }

    #[test]
    fn missed_alert_older_than_window_is_skipped() {
        let mut f = Fired::default();
        assert!(due(&days(), my(4, 20, 58), 10, &mut f).is_empty()); // Maghrib +2 h, before Isyak's reminder
    }

    #[test]
    fn fired_keeps_three_days() {
        let d = days();
        let mut f = Fired::default();
        for day in 1..=6 {
            due(&d, my(day, 18, 58), 0, &mut f);
        }
        assert_eq!(f.keys.len(), 3);
        assert!(f.keys.iter().all(|k| k.starts_with("2026-11-04") || k.starts_with("2026-11-05") || k.starts_with("2026-11-06")));
    }

    #[test]
    fn next_wake_is_next_reminder_or_prayer() {
        let d = days();
        assert_eq!(next_wake(&d, my(4, 17, 0), 10), Some(my(4, 18, 48)));
        assert_eq!(next_wake(&d, my(4, 18, 50), 10), Some(my(4, 18, 58)));
        assert_eq!(next_wake(&d, my(4, 18, 50), 0), Some(my(4, 18, 58)));
    }

    struct OneCall(std::cell::RefCell<Vec<String>>, Result<String, crate::source::RunError>);
    impl Runner for OneCall {
        fn run(&self, program: &str, args: &[&str], _t: std::time::Duration) -> Result<String, crate::source::RunError> {
            self.0.borrow_mut().push(format!("{program} {}", args.join(" ")));
            self.1.clone()
        }
    }

    #[test]
    fn cache_path_includes_zone_and_month() {
        let p = cache_path(Path::new("C:/x"), "WLY02", (2026, 11));
        assert_eq!(p, Path::new("C:/x").join("prayer-WLY02-2026-11.json"));
    }

    #[test]
    fn fetch_uses_curl_with_zone_and_month() {
        let r = OneCall(Default::default(), Ok(FIXTURE.to_string()));
        assert_eq!(fetch_month(&r, "WLY02", (2026, 12)).as_deref(), Some(FIXTURE));
        assert_eq!(fetch_month(&r, "WLY02", (2028, 2)).as_deref(), Some(FIXTURE));
        let calls = r.0.borrow();
        let url = "https://www.e-solat.gov.my/index.php?r=esolatApi/TakwimSolat&period=duration&zone=WLY02";
        assert_eq!(calls[0], format!("curl.exe -s -f -m 30 -A ClaudeNotch -d datestart=2026-12-01&dateend=2026-12-31 {url}"));
        assert_eq!(calls[1], format!("curl.exe -s -f -m 30 -A ClaudeNotch -d datestart=2028-02-01&dateend=2028-02-29 {url}"));
    }

    #[test]
    fn fetch_rejects_bad_body() {
        let r = OneCall(Default::default(), Ok("<html>maintenance</html>".into()));
        assert_eq!(fetch_month(&r, "WLY01", (2026, 11)), None);
        let r = OneCall(Default::default(), Err(crate::source::RunError::Timeout));
        assert_eq!(fetch_month(&r, "WLY01", (2026, 11)), None);
    }

    #[test]
    fn today_missing_is_unavailable() {
        let dir = std::env::temp_dir().join(format!("notch-prayer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(load_days(&dir, "WLY01", my(4, 12, 0)).is_empty());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(cache_path(&dir, "WLY01", (2026, 11)), FIXTURE).unwrap();
        let d = load_days(&dir, "WLY01", my(4, 12, 0));
        assert!(today(&d, my(4, 12, 0)).is_some());
        assert!(load_days(&dir, "WLY02", my(4, 12, 0)).is_empty()); // another zone's month is not used
        let _ = std::fs::remove_dir_all(&dir);
    }
}
