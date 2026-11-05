#!/bin/sh
# Installs or updates Claude Notch on a Mac (experimental). In Terminal, run:
#   curl -fsSL https://raw.githubusercontent.com/hanifmuhd/notch/main/install.sh | sh
# curl leaves no "downloaded from the internet" mark, so macOS opens the app without a prompt.
set -eu
url=https://github.com/hanifmuhd/notch/releases/latest/download/ClaudeNotch-mac.zip
app="Claude Notch.app"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "Downloading Claude Notch..."
curl -fL --progress-bar -o "$tmp/notch.zip" "$url"
ditto -x -k "$tmp/notch.zip" "$tmp"
[ -d "$tmp/$app" ] || { echo "The download did not contain $app." >&2; exit 1; }

dest=/Applications
[ -w "$dest" ] || { dest="$HOME/Applications"; mkdir -p "$dest"; }
# Quit the running copy so it can be replaced.
pkill -x claude-notch 2>/dev/null || pkill -x "Claude Notch" 2>/dev/null || true
sleep 1
rm -rf "$dest/$app"
mv "$tmp/$app" "$dest/$app"
open "$dest/$app"
echo "Claude Notch is installed in $dest and running in the menu bar."
