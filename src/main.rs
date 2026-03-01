use std::collections::{HashMap, HashSet};
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_WATCH_DIR: &str = "/Users/tuan/Library/CloudStorage/Dropbox/screenshots";
const GEMINI_FLASH_MODEL: &str = "flash";

#[derive(Debug, Clone)]
struct Config {
    watch_dir: PathBuf,
    verbose: bool,
    process_existing: bool,
    use_gemini_summary: bool,
    debug_gemini_prompt: bool,
    poll_interval: Duration,
}

#[derive(Debug)]
struct Context {
    seen: HashSet<PathBuf>,
    recently_processed: HashMap<PathBuf, Instant>,
    swift_helper: PathBuf,
    swift_binary: PathBuf,
    swift_module_cache: PathBuf,
}

fn main() -> Result<(), String> {
    let cfg = parse_args(env::args().collect())?;
    if !cfg.watch_dir.exists() {
        return Err(format!(
            "Watch directory does not exist: {}",
            cfg.watch_dir.display()
        ));
    }

    let exe_path = env::current_exe().map_err(|e| e.to_string())?;
    let base_dir = resolve_base_dir(&exe_path);
    let cache_dir = base_dir.join(".cache");
    let mut ctx = Context {
        seen: HashSet::new(),
        recently_processed: HashMap::new(),
        swift_helper: base_dir.join("mac_ocr_vision.swift"),
        swift_binary: cache_dir.join("mac_ocr_vision"),
        swift_module_cache: cache_dir.join("swift-module-cache"),
    };

    if cfg.verbose {
        log_event(
            "startup",
            &format!(
                "watch_dir={} mode=polling poll_interval_ms={} use_gemini_summary={} debug_gemini_prompt={}",
                cfg.watch_dir.display(),
                cfg.poll_interval.as_millis(),
                cfg.use_gemini_summary,
                cfg.debug_gemini_prompt
            ),
        );
        log_event("startup", &format!("base_dir={}", base_dir.display()));
        log_event("startup", &format!("ocr_helper={}", ctx.swift_helper.display()));
        log_event("startup", &format!("ocr_binary={}", ctx.swift_binary.display()));
    }

    seed_seen(&cfg, &mut ctx)?;

    loop {
        let entries = list_supported_files(&cfg.watch_dir)?;
        let current: HashSet<PathBuf> = entries.iter().cloned().collect();

        for path in entries {
            if !ctx.seen.contains(&path) {
                if let Some(dest) = process_path(&cfg, &mut ctx, &path) {
                    ctx.seen.insert(dest);
                } else {
                    ctx.seen.insert(path);
                }
            }
        }

        ctx.seen = current.union(&ctx.seen).cloned().collect();
        prune_recently_processed(&mut ctx.recently_processed);
        thread::sleep(cfg.poll_interval);
    }
}

fn resolve_base_dir(exe_path: &Path) -> PathBuf {
    if let Ok(home) = env::var("SCREENSHOT_RENAMER_HOME") {
        let p = PathBuf::from(home);
        if p.join("mac_ocr_vision.swift").exists() {
            return p;
        }
    }

    if let Ok(cwd) = env::current_dir() {
        if cwd.join("mac_ocr_vision.swift").exists() {
            return cwd;
        }
    }

    let exe_dir = exe_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    if exe_dir.join("mac_ocr_vision.swift").exists() {
        return exe_dir;
    }

    let repo_guess = exe_dir.join("../..");
    if repo_guess.join("mac_ocr_vision.swift").exists() {
        return repo_guess;
    }

    exe_dir
}

fn parse_args(args: Vec<String>) -> Result<Config, String> {
    let mut watch_dir = PathBuf::from(DEFAULT_WATCH_DIR);
    let mut verbose = false;
    let mut process_existing = false;
    let mut use_gemini_summary = false;
    let mut debug_gemini_prompt = false;
    let mut poll_interval = Duration::from_millis(1000);

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--watch-dir" => {
                i += 1;
                if i >= args.len() {
                    return Err("Missing value for --watch-dir".to_string());
                }
                watch_dir = PathBuf::from(&args[i]);
            }
            "--verbose" => verbose = true,
            "--process-existing" => process_existing = true,
            "--use-gemini-summary" => use_gemini_summary = true,
            "--no-gemini-summary" => use_gemini_summary = false,
            "--debug-gemini-prompt" => debug_gemini_prompt = true,
            "--polling" => {
                // Kept for backward compatibility with the Python script flags.
            }
            "--poll-interval-ms" => {
                i += 1;
                if i >= args.len() {
                    return Err("Missing value for --poll-interval-ms".to_string());
                }
                let millis = args[i]
                    .parse::<u64>()
                    .map_err(|_| "Invalid --poll-interval-ms value".to_string())?;
                poll_interval = Duration::from_millis(millis.max(200));
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            other => return Err(format!("Unknown argument: {other}")),
        }
        i += 1;
    }

    Ok(Config {
        watch_dir,
        verbose,
        process_existing,
        use_gemini_summary,
        debug_gemini_prompt,
        poll_interval,
    })
}

fn print_help() {
    println!("Screenshot Auto Renamer (Rust)");
    println!();
    println!("Options:");
    println!("  --watch-dir <path>        Directory to watch");
    println!("  --verbose                 Print detailed logs");
    println!("  --process-existing        Process existing files on startup");
    println!("  --use-gemini-summary      Enable Gemini summary (default: off)");
    println!("  --no-gemini-summary       Disable Gemini summary");
    println!("  --debug-gemini-prompt     Print Gemini prompt before each request");
    println!("  --polling                 No-op compatibility flag");
    println!("  --poll-interval-ms <n>    Poll interval in milliseconds (min 200)");
}

fn seed_seen(cfg: &Config, ctx: &mut Context) -> Result<(), String> {
    let files = list_supported_files(&cfg.watch_dir)?;
    if cfg.process_existing {
        for file in files {
            if let Some(dest) = process_path(cfg, ctx, &file) {
                ctx.seen.insert(dest);
            } else {
                ctx.seen.insert(file);
            }
        }
    } else {
        for file in files {
            ctx.seen.insert(file);
        }
    }
    Ok(())
}

fn process_path(cfg: &Config, ctx: &mut Context, path: &Path) -> Option<PathBuf> {
    if !is_cleanshot_name(path) {
        if cfg.verbose {
            log_event("skip", &format!("reason=not_cleanshot path={}", path.display()));
        }
        return None;
    }

    if is_already_renamed(path) {
        if cfg.verbose {
            log_event(
                "skip",
                &format!("reason=already_renamed_pattern path={}", path.display()),
            );
        }
        return None;
    }

    if should_skip_recent(path, &ctx.recently_processed) {
        return None;
    }
    if !wait_until_stable(path, Duration::from_secs(8)) {
        if cfg.verbose {
            log_event("skip", &format!("reason=file_not_stable path={}", path.display()));
        }
        return None;
    }

    let app_raw = frontmost_app();
    let app = slugify(&app_raw, 40);
    let title = slugify(&frontmost_window_title_with_fallback(&app_raw), 60);
    let ocr_raw = extract_ocr_text(ctx, path);
    let mut ocr = slugify(&ocr_raw, 80);
    if ocr == "unknown" {
        ocr = "no_ocr".to_string();
    }

    let has_vision_ocr = ocr != "no_ocr";
    let mut summary = if has_vision_ocr {
        ocr.clone()
    } else {
        format!("Screenshot_{}", timestamp_compact())
    };
    let prompt_preview = build_gemini_prompt(
        &app.replace('_', " "),
        &title.replace('_', " "),
        if ocr_raw.is_empty() { "no ocr" } else { &ocr_raw },
    );
    if cfg.debug_gemini_prompt && (!cfg.use_gemini_summary || !has_vision_ocr) {
        log_event(
            "gemini_prompt",
            &format!("text={}", compact_for_log(&prompt_preview, 420)),
        );
        if !cfg.use_gemini_summary {
            log_event("gemini_debug", "call_skipped=summary_disabled");
        } else {
            log_event("gemini_debug", "call_skipped=no_ocr");
        }
    }

    if cfg.use_gemini_summary && has_vision_ocr {
        let summary_raw = summarize_with_gemini(
            &app.replace('_', " "),
            &title.replace('_', " "),
            if ocr_raw.is_empty() { "no ocr" } else { &ocr_raw },
            cfg.debug_gemini_prompt,
        );
        let summary_slug = slugify(&summary_raw, 80);
        let gemini_unusable = summary_slug == "unknown" || summary_slug == "no_ocr";
        if !gemini_unusable {
            summary = summary_slug;
        } else {
            // Gemini gave no better summary; keep OCR-based summary.
            summary = ocr.clone();
        }
    }

    let ext = path
        .extension()
        .and_then(OsStr::to_str)
        .map(|s| format!(".{}", s))
        .unwrap_or_default();
    let new_name = build_filename(&app, &title, &summary, &ext);
    let destination = unique_destination(&path.with_file_name(new_name));
    if destination == path {
        return None;
    }

    match fs::rename(path, &destination) {
        Ok(_) => {
            ctx.recently_processed
                .insert(destination.clone(), Instant::now());
            if cfg.verbose {
                log_event(
                    "renamed",
                    &format!("from={} to={}", path.display(), destination.display()),
                );
            }
            Some(destination)
        }
        Err(err) => {
            if cfg.verbose {
                if err.kind() == ErrorKind::NotFound {
                    log_event(
                        "skip",
                        &format!("reason=file_disappeared_before_rename path={}", path.display()),
                    );
                } else {
                    log_event(
                        "error",
                        &format!("action=rename path={} error={}", path.display(), err),
                    );
                }
            }
            None
        }
    }
}

fn is_already_renamed(path: &Path) -> bool {
    match path.file_stem().and_then(OsStr::to_str) {
        Some(stem) => stem.matches("__").count() >= 2,
        None => false,
    }
}

fn is_cleanshot_name(path: &Path) -> bool {
    match path.file_name().and_then(OsStr::to_str) {
        Some(name) => name.starts_with("CleanShot "),
        None => false,
    }
}

fn should_skip_recent(path: &Path, recently_processed: &HashMap<PathBuf, Instant>) -> bool {
    if let Some(last) = recently_processed.get(path) {
        return last.elapsed() < Duration::from_secs(2);
    }
    false
}

fn prune_recently_processed(recently_processed: &mut HashMap<PathBuf, Instant>) {
    recently_processed.retain(|_, ts| ts.elapsed() < Duration::from_secs(10));
}

fn list_supported_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    let read = fs::read_dir(dir).map_err(|e| e.to_string())?;
    for item in read {
        let entry = item.map_err(|e| e.to_string())?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if is_supported_image(&path) {
            out.push(path);
        }
    }
    Ok(out)
}

fn is_supported_image(path: &Path) -> bool {
    match path.extension().and_then(OsStr::to_str) {
        Some(ext) => matches!(
            ext.to_ascii_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "webp" | "tif" | "tiff" | "heic"
        ),
        None => false,
    }
}

fn wait_until_stable(path: &Path, timeout: Duration) -> bool {
    let start = Instant::now();
    let mut previous_size = 0_u64;
    let mut stable_cycles = 0_u8;

    while start.elapsed() < timeout {
        let meta = match fs::metadata(path) {
            Ok(m) => m,
            Err(_) => {
                thread::sleep(Duration::from_millis(200));
                continue;
            }
        };
        let size = meta.len();
        if size > 0 && size == previous_size {
            stable_cycles += 1;
            if stable_cycles >= 3 {
                return true;
            }
        } else {
            previous_size = size;
            stable_cycles = 0;
        }
        thread::sleep(Duration::from_millis(200));
    }

    path.exists()
}

fn frontmost_app() -> String {
    let script = r#"tell application "System Events" to get name of first process whose frontmost is true"#;
    let out = run_osascript(script);
    if out.is_empty() {
        "UnknownApp".to_string()
    } else {
        out
    }
}

fn frontmost_window_title_with_fallback(front_app_name: &str) -> String {
    let ax_title = frontmost_window_title_ax();
    if !ax_title.is_empty() && !is_placeholder_title(&slugify(&ax_title, 60)) {
        return ax_title;
    }

    let tab_title = browser_tab_title(front_app_name);
    if !tab_title.is_empty() {
        return tab_title;
    }

    if ax_title.is_empty() {
        "UntitledWindow".to_string()
    } else {
        ax_title
    }
}

fn frontmost_window_title_ax() -> String {
    let script = r#"
tell application "System Events"
    tell (first process whose frontmost is true)
        try
            set winTitle to value of attribute "AXTitle" of front window
            if winTitle is missing value then
                return ""
            end if
            return winTitle as text
        on error
            return ""
        end try
    end tell
end tell
"#;
    let out = run_osascript(script);
    if out.is_empty() {
        "UntitledWindow".to_string()
    } else {
        out
    }
}

fn browser_tab_title(front_app_name: &str) -> String {
    // Best-effort app-specific browser scripting. This is generally more reliable
    // than AXTitle for browser tabs on macOS.
    let app_lc = front_app_name.to_ascii_lowercase();

    let chromium_like = [
        "google chrome",
        "google chrome canary",
        "chromium",
        "brave browser",
        "microsoft edge",
        "microsoft edge beta",
        "microsoft edge dev",
        "microsoft edge canary",
        "opera",
        "opera one",
        "vivaldi",
        "arc",
        "dia",
        "dia browser",
        "helium",
    ];
    let webkit_like = [
        "safari",
        "safari technology preview",
        "webkit",
        "orion",
    ];
    let firefox_like = ["firefox", "firefox developer edition", "firefox nightly"];

    let family = if chromium_like.iter().any(|x| *x == app_lc) {
        "chromium"
    } else if webkit_like.iter().any(|x| *x == app_lc) {
        "webkit"
    } else if firefox_like.iter().any(|x| *x == app_lc) {
        "firefox"
    } else {
        ""
    };

    if family.is_empty() {
        return String::new();
    }

    let script = if family == "chromium" {
        format!(
            r#"
tell application "{front_app_name}"
    if (count of windows) = 0 then return ""
    try
        return (title of active tab of front window) as text
    on error
        try
            return (name of active tab of front window) as text
        on error
            return ""
        end try
    end try
end tell
"#
        )
    } else if family == "webkit" {
        format!(
            r#"
tell application "{front_app_name}"
    if (count of windows) = 0 then return ""
    try
        return (name of current tab of front window) as text
    on error
        try
            return (name of front document) as text
        on error
            return ""
        end try
    end try
end tell
"#
        )
    } else {
        // Firefox support is limited; best-effort only.
        format!(
            r#"
tell application "{front_app_name}"
    if (count of windows) = 0 then return ""
    try
        return (name of front window) as text
    on error
        return ""
    end try
end tell
"#
        )
    };

    run_osascript(&script)
}

fn run_osascript(script: &str) -> String {
    let output = Command::new("osascript").arg("-e").arg(script).output();
    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        _ => String::new(),
    }
}

fn extract_ocr_text(ctx: &Context, image_path: &Path) -> String {
    if !ensure_swift_ocr_binary(ctx) {
        return String::new();
    }
    let output = Command::new(&ctx.swift_binary).arg(image_path).output();
    match output {
        Ok(out) if out.status.success() => clean_text(&String::from_utf8_lossy(&out.stdout), 10, 90),
        _ => String::new(),
    }
}

fn ensure_swift_ocr_binary(ctx: &Context) -> bool {
    if ctx.swift_binary.exists() {
        let helper_newer = fs::metadata(&ctx.swift_helper)
            .and_then(|m| m.modified())
            .ok()
            .zip(fs::metadata(&ctx.swift_binary).and_then(|m| m.modified()).ok())
            .map(|(helper, bin)| helper > bin)
            .unwrap_or(false);
        if !helper_newer {
            return true;
        }
    }
    if !ctx.swift_helper.exists() {
        return false;
    }
    if fs::create_dir_all(ctx.swift_binary.parent().unwrap_or(Path::new("."))).is_err() {
        return false;
    }
    if fs::create_dir_all(&ctx.swift_module_cache).is_err() {
        return false;
    }

    let output = Command::new("xcrun")
        .arg("swiftc")
        .arg("-O")
        .arg(&ctx.swift_helper)
        .arg("-o")
        .arg(&ctx.swift_binary)
        .env("SWIFT_MODULE_CACHE_PATH", &ctx.swift_module_cache)
        .output();

    matches!(output, Ok(out) if out.status.success() && ctx.swift_binary.exists())
}

fn summarize_with_gemini(app: &str, title: &str, ocr_text: &str, debug_prompt: bool) -> String {
    if which("gemini").is_none() {
        if debug_prompt {
            log_event("gemini_response_error", "gemini_not_found_in_path");
        }
        return String::new();
    }
    let prompt = build_gemini_prompt(app, title, ocr_text);
    if debug_prompt {
        log_event(
            "gemini_prompt",
            &format!("text={}", compact_for_log(&prompt, 420)),
        );
    }

    let output = Command::new("gemini")
        .arg("-p")
        .arg(prompt)
        .arg("-m")
        .arg(GEMINI_FLASH_MODEL)
        .arg("--output-format")
        .arg("text")
        .output();

    let stdout = match output {
        Ok(out) if out.status.success() => {
            let raw = String::from_utf8_lossy(&out.stdout).to_string();
            if debug_prompt {
                log_event(
                    "gemini_response_raw",
                    &format!("text={}", compact_for_log(raw.trim(), 420)),
                );
            }
            raw
        }
        Ok(out) => {
            if debug_prompt {
                let err = String::from_utf8_lossy(&out.stderr);
                log_event(
                    "gemini_response_error",
                    &format!(
                        "exit={} stderr={}",
                        out.status.code().unwrap_or(-1),
                        compact_for_log(err.trim(), 260)
                    ),
                );
            }
            return String::new();
        }
        Err(err) => {
            if debug_prompt {
                log_event("gemini_response_error", &format!("spawn_failed={err}"));
            }
            return String::new();
        }
    };

    let from_json = extract_text_from_gemini_json(&stdout);
    if !from_json.is_empty() {
        let cleaned = clean_text(&from_json, 8, 80);
        if debug_prompt {
            log_event("gemini_response_parsed", &format!("text={}", compact_for_log(&cleaned, 120)));
        }
        return cleaned;
    }
    let cleaned = clean_text(&stdout, 8, 80);
    if debug_prompt {
        log_event(
            "gemini_response_fallback",
            &format!("text={}", compact_for_log(&cleaned, 120)),
        );
    }
    cleaned
}

fn build_gemini_prompt(app: &str, title: &str, ocr_text: &str) -> String {
    let ocr_has_content = !ocr_text.trim().eq_ignore_ascii_case("no ocr");
    let no_ocr_rule = if ocr_has_content {
        "- OCR text is provided, so NEVER output: no ocr.\n"
    } else {
        "- OCR text is empty, so output exactly: no ocr\n"
    };

    format!(
        "Return only a short output name segment for a screenshot filename.\n\
Rules:\n\
- Output only the name segment (no extension).\n\
- Exactly one line, 3 to 8 words.\n\
- Lowercase words separated by spaces.\n\
- No quotes, no labels, no punctuation.\n\
{no_ocr_rule}\n\
App: {app}\n\
Window title: {title}\n\
OCR text: {ocr_text}\n"
    )
}

fn which(cmd: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for dir in env::split_paths(&path) {
        let p = dir.join(cmd);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn extract_text_from_gemini_json(raw: &str) -> String {
    // Minimal extraction without external JSON dependencies.
    // Prefer first `"text":"..."` occurrence.
    let marker = "\"text\"";
    let mut start_idx = 0usize;
    while let Some(rel_idx) = raw[start_idx..].find(marker) {
        let idx = start_idx + rel_idx;
        let after = &raw[idx + marker.len()..];
        if let Some(colon) = after.find(':') {
            let value_part = after[colon + 1..].trim_start();
            if let Some(stripped) = value_part.strip_prefix('"') {
                let mut out = String::new();
                let mut escaped = false;
                for ch in stripped.chars() {
                    if escaped {
                        match ch {
                            'n' => out.push(' '),
                            'r' | 't' => out.push(' '),
                            '"' => out.push('"'),
                            '\\' => out.push('\\'),
                            other => out.push(other),
                        }
                        escaped = false;
                        continue;
                    }
                    if ch == '\\' {
                        escaped = true;
                        continue;
                    }
                    if ch == '"' {
                        return out.trim().to_string();
                    }
                    out.push(ch);
                }
            }
        }
        start_idx = idx + marker.len();
        if start_idx >= raw.len() {
            break;
        }
    }
    String::new()
}

fn clean_text(text: &str, max_words: usize, max_len: usize) -> String {
    let mut out = String::new();
    let mut prev_space = false;
    for ch in text.replace(['\n', '\r'], " ").chars() {
        let allowed = ch.is_ascii_alphanumeric() || ch == ' ' || ch == '_' || ch == '.' || ch == '-';
        if !allowed {
            continue;
        }
        if ch == ' ' {
            if prev_space {
                continue;
            }
            prev_space = true;
        } else {
            prev_space = false;
        }
        out.push(ch);
        if out.len() >= max_len {
            break;
        }
    }
    let words: Vec<&str> = out.split_whitespace().take(max_words).collect();
    words.join(" ").trim().to_string()
}

fn slugify(text: &str, max_len: usize) -> String {
    let cleaned = clean_text(text, 20, max_len);
    if cleaned.is_empty() {
        return "unknown".to_string();
    }
    let mut out = cleaned.replace(' ', "_");
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    out = out.trim_matches(|c: char| c == '_' || c == '.' || c == '-').to_string();
    if out.is_empty() {
        "unknown".to_string()
    } else {
        out
    }
}

fn unique_destination(candidate: &Path) -> PathBuf {
    if !candidate.exists() {
        return candidate.to_path_buf();
    }
    let parent = candidate.parent().unwrap_or(Path::new("."));
    let stem = candidate
        .file_stem()
        .and_then(OsStr::to_str)
        .unwrap_or("file")
        .to_string();
    let ext = candidate
        .extension()
        .and_then(OsStr::to_str)
        .map(|e| format!(".{e}"))
        .unwrap_or_default();
    for i in 2..10000 {
        let next = parent.join(format!("{stem}_{i}{ext}"));
        if !next.exists() {
            return next;
        }
    }
    candidate.to_path_buf()
}

fn build_filename(app: &str, title: &str, summary: &str, ext: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push(app.to_string());
    if !is_placeholder_title(title) {
        parts.push(title.to_string());
    }
    parts.push(summary.to_string());
    format!("{}{}", parts.join("__"), ext)
}

fn is_placeholder_title(title: &str) -> bool {
    title.eq_ignore_ascii_case("UntitledWindow")
        || title.eq_ignore_ascii_case("Untitled")
        || title.eq_ignore_ascii_case("unknown")
}

fn log_event(event: &str, message: &str) {
    println!("{} event={} {}", now_ts(), event, compact_for_log(message, 1200));
}

fn now_ts() -> String {
    let output = Command::new("date").arg("+%Y-%m-%d %H:%M:%S").output();
    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        _ => "0000-00-00 00:00:00".to_string(),
    }
}

fn timestamp_compact() -> String {
    let output = Command::new("date").arg("+%Y%m%d_%H%M%S").output();
    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        _ => "00000000_000000".to_string(),
    }
}

fn compact_for_log(text: &str, max_len: usize) -> String {
    let mut out = text.replace('\n', "\\n").replace('\r', "\\r");
    out = out.split_whitespace().collect::<Vec<&str>>().join(" ");
    if out.len() > max_len {
        out.truncate(max_len);
        out.push_str("...");
    }
    out
}
