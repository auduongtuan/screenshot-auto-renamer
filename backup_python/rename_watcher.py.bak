#!/usr/bin/env python3
"""
Watch a folder for new images and rename them using:
1) frontmost application
2) front window title
3) OCR text from the image
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import time
from pathlib import Path

try:
    from watchdog.events import FileSystemEventHandler
    from watchdog.observers import Observer
except Exception:  # pragma: no cover
    FileSystemEventHandler = object  # type: ignore[assignment]
    Observer = None  # type: ignore[assignment]

SUPPORTED_EXTENSIONS = {".png", ".jpg", ".jpeg", ".webp", ".tif", ".tiff", ".heic"}
SCRIPT_DIR = Path(__file__).resolve().parent
SWIFT_HELPER = SCRIPT_DIR / "mac_ocr_vision.swift"
SWIFT_BINARY = SCRIPT_DIR / ".cache" / "mac_ocr_vision"
SWIFT_MODULE_CACHE = SCRIPT_DIR / ".cache" / "swift-module-cache"
GEMINI_FLASH_MODEL = "gemini-2.5-flash"


def run_osascript(script: str) -> str:
    result = subprocess.run(
        ["osascript", "-e", script],
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        return ""
    return result.stdout.strip()


def get_frontmost_app() -> str:
    script = 'tell application "System Events" to get name of first process whose frontmost is true'
    value = run_osascript(script)
    return value or "UnknownApp"


def get_frontmost_window_title() -> str:
    script = """
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
"""
    value = run_osascript(script)
    return value or "UntitledWindow"


def clean_text(text: str, max_words: int = 8, max_len: int = 80) -> str:
    text = text.replace("\n", " ").replace("\r", " ")
    text = re.sub(r"\s+", " ", text).strip()
    text = re.sub(r"[^A-Za-z0-9 _.-]", "", text)
    if not text:
        return ""
    words = text.split(" ")
    text = " ".join(words[:max_words]).strip()
    text = text[:max_len].strip()
    return text


def slugify(text: str, max_len: int = 80) -> str:
    text = clean_text(text, max_words=20, max_len=max_len)
    text = text.replace(" ", "_")
    text = re.sub(r"_+", "_", text)
    text = text.strip("_.-")
    return text or "unknown"


def ensure_swift_ocr_binary() -> bool:
    if SWIFT_BINARY.exists():
        return True

    if not SWIFT_HELPER.exists():
        return False

    SWIFT_BINARY.parent.mkdir(parents=True, exist_ok=True)
    SWIFT_MODULE_CACHE.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env["SWIFT_MODULE_CACHE_PATH"] = str(SWIFT_MODULE_CACHE)

    result = subprocess.run(
        [
            "xcrun",
            "swiftc",
            "-O",
            str(SWIFT_HELPER),
            "-o",
            str(SWIFT_BINARY),
        ],
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )
    return result.returncode == 0 and SWIFT_BINARY.exists()


def extract_ocr_text(image_path: Path) -> str:
    if not ensure_swift_ocr_binary():
        return ""

    try:
        result = subprocess.run(
            [str(SWIFT_BINARY), str(image_path)],
            capture_output=True,
            text=True,
            check=False,
        )
        if result.returncode != 0:
            return ""
        return clean_text(result.stdout, max_words=10, max_len=90)
    except Exception:
        return ""


def _extract_text_from_gemini_json(stdout_text: str) -> str:
    try:
        payload = json.loads(stdout_text.strip())
    except Exception:
        return ""

    if isinstance(payload, dict):
        for key in ("text", "output", "response"):
            value = payload.get(key)
            if isinstance(value, str):
                return value.strip()

        candidates = payload.get("candidates")
        if isinstance(candidates, list):
            for candidate in candidates:
                if not isinstance(candidate, dict):
                    continue
                content = candidate.get("content")
                if not isinstance(content, dict):
                    continue
                parts = content.get("parts")
                if not isinstance(parts, list):
                    continue
                for part in parts:
                    if isinstance(part, dict) and isinstance(part.get("text"), str):
                        return part["text"].strip()
    return ""


def summarize_with_gemini(app: str, title: str, ocr_text: str, timeout_sec: int = 15) -> str:
    gemini_path = shutil.which("gemini")
    if not gemini_path:
        return ""

    prompt = (
        "Create a short screenshot filename summary.\n"
        "Rules:\n"
        "- Return only one line.\n"
        "- 3 to 8 words.\n"
        "- Use plain English words only.\n"
        "- No punctuation except spaces.\n"
        "- If content is unclear, return: no ocr\n\n"
        f"App: {app}\n"
        f"Window title: {title}\n"
        f"OCR text: {ocr_text}\n"
    )

    result = subprocess.run(
        [
            gemini_path,
            "-p",
            prompt,
            "-m",
            GEMINI_FLASH_MODEL,
            "--output-format",
            "json",
        ],
        capture_output=True,
        text=True,
        check=False,
        timeout=timeout_sec,
    )
    if result.returncode != 0:
        return ""

    response_text = _extract_text_from_gemini_json(result.stdout)
    if not response_text:
        response_text = result.stdout.strip()
    return clean_text(response_text, max_words=8, max_len=80)


def unique_destination(dest: Path) -> Path:
    if not dest.exists():
        return dest
    stem = dest.stem
    suffix = dest.suffix
    parent = dest.parent
    for i in range(2, 10000):
        candidate = parent / f"{stem}_{i}{suffix}"
        if not candidate.exists():
            return candidate
    raise RuntimeError("Could not produce a unique destination filename.")


def wait_until_stable(path: Path, timeout_sec: float = 8.0) -> bool:
    start = time.time()
    previous_size = -1
    stable_cycles = 0
    while time.time() - start < timeout_sec:
        if not path.exists():
            time.sleep(0.2)
            continue
        current_size = path.stat().st_size
        if current_size > 0 and current_size == previous_size:
            stable_cycles += 1
            if stable_cycles >= 3:
                return True
        else:
            stable_cycles = 0
            previous_size = current_size
        time.sleep(0.2)
    return path.exists()


class ScreenshotHandler(FileSystemEventHandler):
    def __init__(self, watch_dir: Path, verbose: bool = False, use_gemini: bool = True) -> None:
        super().__init__()
        self.watch_dir = watch_dir
        self.verbose = verbose
        self.use_gemini = use_gemini
        self._recently_processed: dict[Path, float] = {}

    def on_created(self, event) -> None:  # type: ignore[override]
        if event.is_directory:
            return
        self._handle(Path(event.src_path))

    def on_moved(self, event) -> None:  # type: ignore[override]
        if event.is_directory:
            return
        self._handle(Path(event.dest_path))

    def _handle(self, path: Path) -> None:
        ext = path.suffix.lower()
        if ext not in SUPPORTED_EXTENSIONS:
            return

        # Skip files we touched recently to avoid rename loops.
        now = time.time()
        last = self._recently_processed.get(path)
        if last and (now - last) < 2:
            return

        if not wait_until_stable(path):
            if self.verbose:
                print(f"[skip] File not stable: {path.name}")
            return

        app = slugify(get_frontmost_app(), max_len=40)
        title = slugify(get_frontmost_window_title(), max_len=60)
        ocr_raw = extract_ocr_text(path)
        ocr = slugify(ocr_raw, max_len=80)
        if ocr == "unknown":
            ocr = "no_ocr"

        summary = ocr
        if self.use_gemini:
            try:
                summary_raw = summarize_with_gemini(
                    app=app.replace("_", " "),
                    title=title.replace("_", " "),
                    ocr_text=ocr_raw or "no ocr",
                )
                summary_slug = slugify(summary_raw, max_len=80)
                if summary_slug != "unknown":
                    summary = summary_slug
            except Exception as exc:
                if self.verbose:
                    print(f"[warn] Gemini summary failed for {path.name}: {exc}")

        new_name = f"{app}__{title}__{summary}{ext}"
        destination = unique_destination(path.with_name(new_name))

        if destination == path:
            return
        try:
            path.rename(destination)
            self._recently_processed[destination] = time.time()
            if self.verbose:
                print(f"[renamed] {path.name} -> {destination.name}")
        except PermissionError as exc:
            if self.verbose:
                print(f"[error] Permission denied renaming {path.name}: {exc}")
        except OSError as exc:
            if self.verbose:
                print(f"[error] Rename failed for {path.name}: {exc}")


def poll_loop(watch_dir: Path, handler: ScreenshotHandler) -> None:
    seen: set[Path] = set()
    for p in watch_dir.iterdir():
        if p.is_file():
            seen.add(p)
    if handler.verbose:
        print("watchdog not available, using polling mode (1s interval)")
    while True:
        current: set[Path] = set()
        for p in watch_dir.iterdir():
            if not p.is_file():
                continue
            current.add(p)
            if p not in seen:
                handler._handle(p)
        seen = current
        time.sleep(1)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Auto-rename screenshot files.")
    parser.add_argument(
        "--watch-dir",
        default="/Users/tuan/Library/CloudStorage/Dropbox/screenshots",
        help="Directory to watch for new screenshots.",
    )
    parser.add_argument(
        "--verbose",
        action="store_true",
        help="Print rename events.",
    )
    parser.add_argument(
        "--polling",
        action="store_true",
        help="Force polling mode instead of watchdog observers (recommended for CloudStorage/Dropbox).",
    )
    parser.add_argument(
        "--process-existing",
        action="store_true",
        help="Also process existing images at startup (default: only new files).",
    )
    parser.add_argument(
        "--no-gemini-summary",
        action="store_true",
        help="Disable Gemini summary and use OCR text directly.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    watch_dir = Path(args.watch_dir).expanduser().resolve()

    if not watch_dir.exists():
        print(f"Watch directory does not exist: {watch_dir}")
        return 1

    handler = ScreenshotHandler(
        watch_dir=watch_dir,
        verbose=args.verbose,
        use_gemini=not args.no_gemini_summary,
    )
    if args.process_existing:
        for p in watch_dir.iterdir():
            if p.is_file() and p.suffix.lower() in SUPPORTED_EXTENSIONS:
                try:
                    handler._handle(p)
                except Exception as exc:
                    if args.verbose:
                        print(f"[error] Failed processing existing file {p.name}: {exc}")

    print(f"Watching: {watch_dir}")

    if args.polling or Observer is None:
        try:
            poll_loop(watch_dir=watch_dir, handler=handler)
        except KeyboardInterrupt:
            pass
        return 0

    observer = Observer()
    try:
        observer.schedule(handler, str(watch_dir), recursive=False)
        observer.start()
    except Exception:
        try:
            observer.stop()
        except Exception:
            pass
        if args.verbose:
            print("watchdog observer failed, falling back to polling mode (1s interval)")
        try:
            poll_loop(watch_dir=watch_dir, handler=handler)
        except KeyboardInterrupt:
            pass
        return 0

    try:
        while True:
            time.sleep(1)
    except KeyboardInterrupt:
        observer.stop()
    observer.join()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
