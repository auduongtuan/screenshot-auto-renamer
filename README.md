# Screenshot Auto Renamer (macOS)

Watches this folder:

`/Users/tuan/Library/CloudStorage/Dropbox/screenshots`

When a new image appears, it renames the file to:

`{frontmost_app}__{window_title}__{gemini_summary}.{ext}`

Example:

`Google_Chrome__ChatGPT_-_Project_Notes__fix_api_timeout_in_create_order.png`

## Requirements

- macOS
- Python 3.10+
- Xcode Command Line Tools (`xcode-select --install`)
- Apple Vision framework (built into macOS)
- Gemini CLI (for summary generation with Gemini Flash)

Install Python environment:

```bash
cd /Users/tuan/Code/screenshot_auto_renamer
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
```

Note:
- OCR uses native macOS `Vision` via a small Swift helper.
- On first run, the script auto-compiles the helper with `swiftc`.
- If `pip install` cannot access internet, the script still runs using built-in polling mode.

Install Gemini CLI (official):

```bash
npm install -g @google/gemini-cli
gemini
```

When enabled, the script calls Gemini CLI with:

`gemini -m gemini-2.5-flash --output-format json`

Gemini is OFF by default. Turn it on with `--use-gemini-summary`.

## Run (Rust)

```bash
cd /Users/tuan/Code/screenshot_auto_renamer
cargo run --release -- --verbose --polling
```

Recommended for Dropbox/CloudStorage folders:

```bash
./target/release/screenshot_auto_renamer --verbose --polling
```

Print Gemini prompt for debugging:

```bash
./target/release/screenshot_auto_renamer --verbose --polling --use-gemini-summary --debug-gemini-prompt
```

## Permissions (Important)

The script uses AppleScript/System Events to read frontmost app + window title.
Grant permissions if prompted:

- System Settings -> Privacy & Security -> Accessibility
  - Enable Terminal (or your shell app)
- System Settings -> Privacy & Security -> Automation
  - Allow Terminal to control System Events

Without these permissions, app/window info may be empty and fallback names will be used.

## Run in Background (optional)

You can run it in a `tmux`/`screen` session, or set up a LaunchAgent later.

### LaunchAgent setup (Rust binary)

```bash
cp /Users/tuan/Code/screenshot_auto_renamer/com.tuan.screenshot-auto-renamer.plist ~/Library/LaunchAgents/
launchctl unload ~/Library/LaunchAgents/com.tuan.screenshot-auto-renamer.plist 2>/dev/null || true
launchctl load ~/Library/LaunchAgents/com.tuan.screenshot-auto-renamer.plist
launchctl start com.tuan.screenshot-auto-renamer
```

Check logs:

```bash
tail -f /tmp/screenshot-auto-renamer.log /tmp/screenshot-auto-renamer.err
```

Stop/uninstall:

```bash
launchctl stop com.tuan.screenshot-auto-renamer
launchctl unload ~/Library/LaunchAgents/com.tuan.screenshot-auto-renamer.plist
rm ~/Library/LaunchAgents/com.tuan.screenshot-auto-renamer.plist
```
