#!/usr/bin/env python3
"""Regenerate the CodexManager bundled snapshot; --check compares without writing.

Requires Python 3.9+, the official codex executable, and Cargo with serde_json
(and dependencies from the repository Cargo.lock) cached for offline use. Set
--codex to the native binary to select a specific release. No model is invoked.
source_sha256 hashes the CLI's exact stdout bytes, including the final newline.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile


HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
MODELS = ROOT / "crates/service/src/models_v2"
SNAPSHOT = MODELS / "official_model_capabilities.json"
CATALOG_ARGS = ("debug", "models", "--bundled")


def collect_catalog(binary, tmp):
    # Build from scratch: do not forward API keys, login state, proxy settings,
    # loader overrides, or caller-supplied Codex/agent configuration.
    env = {k: os.environ[k] for k in ("SystemRoot", "WINDIR") if k in os.environ}
    if os.name != "nt":  # Some Unix installations use an env/node launcher.
        env["PATH"] = os.environ.get("PATH", os.defpath)
    for key, subdir in {
        "CODEX_HOME": "codex", "HOME": "home", "USERPROFILE": "home",
        "APPDATA": "roaming", "LOCALAPPDATA": "local", "TMP": "tmp",
        "TEMP": "tmp", "TMPDIR": "tmp", "XDG_CONFIG_HOME": "config",
        "XDG_CACHE_HOME": "cache", "XDG_DATA_HOME": "data", "XDG_STATE_HOME": "state",
    }.items():
        (tmp / subdir).mkdir(exist_ok=True)
        env[key] = str(tmp / subdir)
    cwd = tmp / "empty"
    cwd.mkdir()

    def invoke(*args):
        return subprocess.run([binary, *args], cwd=cwd, env=env,
                              stdin=subprocess.DEVNULL, capture_output=True,
                              timeout=60, check=True).stdout

    version = re.fullmatch(rb"codex-cli ([0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.+-]+)?)",
                           invoke("--version").strip())
    if not version:
        raise ValueError("unexpected codex --version output")
    return version[1].decode("ascii"), invoke(*CATALOG_ARGS)


def project(raw, tmp):
    crate = tmp / "projector"
    (crate / "src").mkdir(parents=True)
    shutil.copyfile(MODELS / "discovery_metadata/sanitizer.rs", crate / "src/sanitizer.rs")
    shutil.copyfile(HERE / "codex-capabilities-projector.rs", crate / "src/main.rs")
    (crate / "Cargo.toml").write_text(
        '[package]\nname="codex-capabilities-projector"\nversion="0.0.0"\nedition="2021"\n'
        '[dependencies]\nserde_json="1"\n[workspace]\n', encoding="utf-8")
    # Cargo prunes a temporary copy. Neither the service workspace nor its
    # lockfile/target directory is built, locked or modified by the projector.
    shutil.copyfile(ROOT / "Cargo.lock", crate / "Cargo.lock")
    result = subprocess.run(["cargo", "run", "--quiet", "--offline", "--target-dir", "target"],
                            cwd=crate, input=raw, stdout=subprocess.PIPE, check=True)
    return [{"slug": m["slug"], "metadata": m["metadata"]} for m in json.loads(result.stdout)]


def refresh(binary, check):
    with tempfile.TemporaryDirectory(prefix="codex-bundled-") as directory:
        tmp = Path(directory).resolve()
        version, raw = collect_catalog(binary, tmp)
        digest = hashlib.sha256(raw).hexdigest()
        models = project(raw, tmp)
    snapshot = {"codex_version": version, "source": "codex " + " ".join(CATALOG_ARGS),
                "source_sha256": digest, "models": models}
    output = json.dumps(snapshot, ensure_ascii=False, indent=2) + "\n"
    try:
        matches = SNAPSHOT.read_text(encoding="utf-8") == output  # allow Git CRLF checkouts
    except (FileNotFoundError, UnicodeDecodeError):
        matches = False
    info = f"codex-cli {version}, {len(models)} models, raw sha256 {digest}"
    if matches:
        print(f"Snapshot matches ({info})")
        return 0
    if check:
        print(f"Snapshot missing or stale ({info}); rerun without --check", file=sys.stderr)
        return 1
    with tempfile.NamedTemporaryFile(dir=SNAPSHOT.parent, delete=False) as f:
        staging = Path(f.name)
    try:
        staging.write_bytes(output.encode("utf-8"))
        if SNAPSHOT.exists():
            shutil.copymode(SNAPSHOT, staging)
        staging.replace(SNAPSHOT)
    finally:
        staging.unlink(missing_ok=True)
    print(f"Refreshed {SNAPSHOT.name} ({info})")
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--check", action="store_true", help="exit 0 if current, 1 if stale; never write")
    parser.add_argument("--codex", default="codex.exe" if os.name == "nt" else "codex",
                        help="path or executable name for the official Codex CLI")
    args = parser.parse_args()
    binary = shutil.which(args.codex)
    if not binary:
        parser.error("codex executable not found; supply --codex PATH")
    if os.name == "nt" and Path(binary).suffix.lower() != ".exe":
        parser.error("supply the native codex.exe (not a shell wrapper)")
    try:
        return refresh(str(Path(binary).resolve()), args.check)
    except (ValueError, OSError, subprocess.SubprocessError) as exc:
        print(f"Refresh failed: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
