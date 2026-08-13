#!/usr/bin/env python3
"""Track upstream protocol changes and copy reusable WebHome assets."""

import argparse
import hashlib
import json
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Dict, Iterable, List, Tuple

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
if hasattr(sys.stderr, "reconfigure"):
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

SCRIPT_DIR = Path(__file__).resolve().parent
DESKTOP_DIR = SCRIPT_DIR.parent
UPSTREAM_DIR = DESKTOP_DIR.parent / "webhtv"
SNAPSHOT_FILE = SCRIPT_DIR / ".upstream_snapshot.json"

CRITICAL_PATHS = (
    "catvod/src/main/java/com/github/catvod/crawler/Spider.java",
    "app/src/main/java/com/fongmi/android/tv/api/loader/BaseLoader.java",
    "app/src/main/java/com/fongmi/android/tv/web/HomeWebBridge.java",
    "app/src/main/java/com/fongmi/android/tv/web/HomeWebController.java",
    "app/src/main/java/com/fongmi/android/tv/bean/Site.java",
    "app/src/main/java/com/fongmi/android/tv/api/config/VodConfig.java",
    "webhome-devkit/docs/应用完整开发文档.md",
)

WARNING_PATHS = (
    "app/src/main/java/com/fongmi/android/tv/server/Nano.java",
    "app/src/main/java/com/fongmi/android/tv/server/Server.java",
    "app/src/main/java/com/fongmi/android/tv/server/process/",
    "app/src/main/java/com/fongmi/android/tv/web/WebCall.java",
    "app/src/main/java/com/fongmi/android/tv/web/CookieBridge.java",
    "app/src/main/java/com/fongmi/android/tv/web/ext/",
    "app/src/main/java/com/fongmi/android/tv/player/engine/PlayerEngine.java",
    "app/src/main/java/com/fongmi/android/tv/bean/Vod.java",
    "app/src/main/java/com/fongmi/android/tv/bean/Parse.java",
    "app/src/main/java/com/fongmi/android/tv/Constant.java",
)

ASSET_MAP = {
    "app/src/main/assets/index.html": "shared/app-assets/index.html",
    "app/src/main/assets/manage.html": "shared/app-assets/manage.html",
    "app/src/main/assets/parse.html": "shared/app-assets/parse.html",
    "app/src/main/assets/css/": "shared/app-assets/css/",
    "app/src/main/assets/js/": "shared/app-assets/js/",
    "app/src/main/assets/lut_presets/": "shared/app-assets/lut_presets/",
    "webhome-devkit/": "shared/webhome-devkit/",
}


def fail(message: str) -> None:
    print("[ERROR] " + message, file=sys.stderr)
    raise SystemExit(1)


def ensure_layout() -> None:
    if not UPSTREAM_DIR.is_dir() or not (UPSTREAM_DIR / ".git").is_dir():
        fail("upstream repository not found: {}".format(UPSTREAM_DIR))


def normalized(path: Path) -> str:
    return str(path.relative_to(UPSTREAM_DIR)).replace("\\", "/")


def hash_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 128), b""):
            digest.update(chunk)
    return digest.hexdigest()


def files_for(relative_path: str) -> Iterable[Path]:
    path = UPSTREAM_DIR / relative_path
    if path.is_file():
        yield path
    elif path.is_dir():
        for child in sorted(path.rglob("*")):
            if child.is_file() and ".git" not in child.parts:
                yield child


def scan() -> Dict[str, str]:
    result: Dict[str, str] = {}
    watched = list(CRITICAL_PATHS) + list(WARNING_PATHS) + list(ASSET_MAP.keys())
    for entry in watched:
        for path in files_for(entry):
            result[normalized(path)] = hash_file(path)
    return result


def git_output(*args: str) -> str:
    process = subprocess.run(
        ["git"] + list(args),
        cwd=str(UPSTREAM_DIR),
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    if process.returncode != 0:
        fail(process.stderr.strip() or "git command failed")
    return process.stdout.strip()


def snapshot_payload(files: Dict[str, str]) -> Dict[str, object]:
    return {
        "upstream": "https://github.com/Silent1566/webhtv",
        "commit": git_output("rev-parse", "HEAD"),
        "files": files,
    }


def save_snapshot(files: Dict[str, str]) -> None:
    payload = snapshot_payload(files)
    SNAPSHOT_FILE.write_text(
        json.dumps(payload, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    print("[OK] snapshot saved: {} files at {}".format(len(files), payload["commit"]))


def load_snapshot() -> Tuple[str, Dict[str, str]]:
    if not SNAPSHOT_FILE.is_file():
        fail("snapshot missing; run --init first")
    payload = json.loads(SNAPSHOT_FILE.read_text(encoding="utf-8"))
    files = payload.get("files")
    if not isinstance(files, dict):
        fail("snapshot format is invalid")
    return str(payload.get("commit", "unknown")), files


def matches(path: str, patterns: Tuple[str, ...]) -> bool:
    return any(path == item.rstrip("/") or path.startswith(item) for item in patterns)


def classification(path: str) -> str:
    if matches(path, CRITICAL_PATHS):
        return "CRITICAL"
    if matches(path, WARNING_PATHS):
        return "WARNING"
    return "ASSET"


def diff(old: Dict[str, str], new: Dict[str, str]) -> List[Tuple[str, str, str]]:
    changes: List[Tuple[str, str, str]] = []
    for path in sorted(set(old) | set(new)):
        if path not in old:
            changes.append((classification(path), "added", path))
        elif path not in new:
            changes.append((classification(path), "deleted", path))
        elif old[path] != new[path]:
            changes.append((classification(path), "modified", path))
    return changes


def check() -> int:
    old_commit, old_files = load_snapshot()
    new_files = scan()
    changes = diff(old_files, new_files)
    current_commit = git_output("rev-parse", "HEAD")
    print("[INFO] snapshot commit: {}".format(old_commit))
    print("[INFO] current commit:  {}".format(current_commit))
    if not changes:
        print("[OK] no monitored upstream changes")
        return 0

    for level in ("CRITICAL", "WARNING", "ASSET"):
        group = [item for item in changes if item[0] == level]
        if not group:
            continue
        print("\n[{}] {} change(s)".format(level, len(group)))
        for _, action, path in group:
            print("  {:8} {}".format(action, path))

    print("\n[INFO] review protocol changes, sync assets, then run --accept")
    return 2 if any(item[0] == "CRITICAL" for item in changes) else 0


def copy_entry(source_relative: str, target_relative: str) -> None:
    source = UPSTREAM_DIR / source_relative
    target = DESKTOP_DIR / target_relative
    if source.is_file():
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(str(source), str(target))
    elif source.is_dir():
        if target.exists():
            shutil.rmtree(str(target))
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copytree(str(source), str(target))
    else:
        fail("asset source missing: {}".format(source))
    print("[SYNC] {} -> {}".format(source_relative, target_relative))


def sync_assets() -> None:
    for source, target in ASSET_MAP.items():
        copy_entry(source, target)
    print("[OK] reusable upstream assets synchronized")


def pull() -> None:
    print(git_output("pull", "--ff-only"))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    action = parser.add_mutually_exclusive_group(required=True)
    action.add_argument("--init", action="store_true", help="create the first snapshot")
    action.add_argument("--check", action="store_true", help="compare local upstream files")
    action.add_argument("--accept", action="store_true", help="accept current files as baseline")
    action.add_argument("--sync-assets", action="store_true", help="copy reusable web assets")
    action.add_argument("--pull", action="store_true", help="fast-forward upstream, then check")
    args = parser.parse_args()

    ensure_layout()
    if args.init or args.accept:
        save_snapshot(scan())
        return 0
    if args.sync_assets:
        sync_assets()
        return 0
    if args.pull:
        pull()
    return check()


if __name__ == "__main__":
    raise SystemExit(main())
