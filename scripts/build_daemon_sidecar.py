#!/usr/bin/env python3
"""Compile the admin daemon (rustatio_daemon.py) into the executable bundled with the desktop app.

The result is a standalone executable (Python and dependencies included) written where Tauri
expects a sidecar: rustatio-desktop/binaries/rustatio-daemon-<target triple>[.exe]

    pip install -r scripts/daemon-requirements.txt
    python scripts/build_daemon_sidecar.py                 # for this machine
    python scripts/build_daemon_sidecar.py --universal     # macOS: arm64 + x86_64 in one file

Run it before `cargo tauri build` (mise run build:desktop does). It has to run on the platform
it builds for: PyInstaller does not cross-compile.
"""

import argparse
import json
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "scripts" / "rustatio_daemon.py"
OUT_DIR = ROOT / "rustatio-desktop" / "binaries"
NAME = "rustatio-daemon"
SMOKE_TIMEOUT = 30


def host_triple():
    """Target triple of this machine, as Rust names it."""
    out = subprocess.run(["rustc", "-vV"], capture_output=True, text=True, check=True).stdout
    for line in out.splitlines():
        if line.startswith("host:"):
            return line.split(":", 1)[1].strip()
    raise SystemExit("Cannot find the host target triple in `rustc -vV`")


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def sidecar_names(triple, universal):
    """File names Tauri looks for: one per target, plus the universal macOS one."""
    suffix = ".exe" if "windows" in triple else ""
    triples = [triple]
    if universal:
        triples = ["universal-apple-darwin", "aarch64-apple-darwin", "x86_64-apple-darwin"]
    return [f"{NAME}-{t}{suffix}" for t in triples]


def is_universal2(path):
    """True when the Mach-O file holds both macOS architectures."""
    try:
        out = subprocess.run(
            ["lipo", "-archs", str(path)], capture_output=True, text=True, check=True
        ).stdout.split()
    except (OSError, subprocess.CalledProcessError):
        return False
    return {"arm64", "x86_64"} <= set(out)


def python_is_universal2():
    """True when the running Python holds both macOS architectures (PyInstaller needs it)."""
    return is_universal2(os.path.realpath(sys.executable))


def thin_packages():
    """Installed packages with a compiled module for one architecture only, as name==version.

    pip prefers the wheel of the machine architecture over the universal2 one, but a universal
    executable needs every compiled module to be universal2."""
    from importlib import metadata

    thin = set()
    for dist in metadata.distributions():
        for file in dist.files or []:
            if file.suffix in (".so", ".dylib") and not is_universal2(dist.locate_file(file)):
                thin.add(f"{dist.metadata['Name']}=={dist.version}")
                break
    return sorted(thin)


def install_universal_wheels():
    """Replace the thin compiled packages by their universal2 wheels. True when none is left."""
    import sysconfig

    thin = thin_packages()
    if not thin:
        return True

    print("universal2 wheels for:", ", ".join(thin))
    cmd = [
        sys.executable, "-m", "pip", "install", "--upgrade", "--no-deps", "--only-binary=:all:",
        "--platform", "macosx_10_13_universal2", "--platform", "macosx_11_0_universal2",
        "--target", sysconfig.get_paths()["purelib"], *thin,
    ]
    if subprocess.run(cmd).returncode != 0:
        return False
    return not thin_packages()


def build(universal):
    if universal and not (python_is_universal2() and install_universal_wheels()):
        # Better an arm64-only daemon (Intel Macs run the app without the Daemon view) than no
        # release at all: say it loudly instead of failing.
        message = "no universal2 Python environment: the daemon is built for this Mac only"
        print(f"::warning::{message}" if os.getenv("GITHUB_ACTIONS") else f"warning: {message}")
        universal = False

    try:
        import PyInstaller  # noqa: F401
    except ImportError:
        raise SystemExit("PyInstaller is missing: pip install -r scripts/daemon-requirements.txt")

    work = Path(tempfile.mkdtemp(prefix="rustatio-daemon-build-"))
    cmd = [
        sys.executable, "-m", "PyInstaller",
        "--noconfirm", "--clean", "--onefile",
        "--name", NAME,
        "--distpath", str(work / "dist"),
        "--workpath", str(work / "build"),
        "--specpath", str(work),
    ]
    if universal:
        cmd += ["--target-arch", "universal2"]
    cmd.append(str(SCRIPT))

    print("+", " ".join(cmd))
    subprocess.run(cmd, check=True)

    built = next((work / "dist").glob(f"{NAME}*"), None)
    if built is None:
        raise SystemExit("PyInstaller did not produce an executable")
    return work, built


def smoke_test(command):
    """Start the daemon like the app does and wait for its admin panel to answer."""
    token = "smoke-test"
    port, admin_port = free_port(), free_port()
    data = Path(tempfile.mkdtemp(prefix="rustatio-daemon-smoke-"))
    (data / "state.json").write_text(json.dumps({"default_config": None}), encoding="utf-8")

    env = {
        **os.environ,
        "PORT": str(port),
        "RUSTATIO_API": f"http://127.0.0.1:{port}",
        "ADMIN_PORT": str(admin_port),
        "ADMIN_HOST": "127.0.0.1",
        "AUTH_TOKEN": token,
        "RULES_FILE": str(data / "rules.txt"),
        "DEFAULTS_FILE": str(data / "state.json"),
        "LOGFILE": str(data / "daemon.log"),
        "CHECK_LOGS_FILE": str(data / "check_logs.json"),
        "ARCHIVE_FOLDER": str(data / "archived"),
        "PYTHONUTF8": "1",
    }
    process = subprocess.Popen(command, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    request = urllib.request.Request(
        f"http://127.0.0.1:{admin_port}/api/status",
        headers={"Authorization": f"Bearer {token}"},
    )

    try:
        deadline = time.time() + SMOKE_TIMEOUT
        while time.time() < deadline:
            if process.poll() is not None:
                return False, f"the daemon exited with code {process.returncode}"
            try:
                with urllib.request.urlopen(request, timeout=2) as response:
                    return response.status == 200, f"admin panel answered HTTP {response.status}"
            except (urllib.error.URLError, OSError):
                time.sleep(0.5)
        return False, f"no answer from the admin panel after {SMOKE_TIMEOUT}s"
    finally:
        process.kill()
        process.wait()
        shutil.rmtree(data, ignore_errors=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--target", help="target triple (default: this machine)")
    parser.add_argument("--universal", action="store_true", help="macOS: universal2 executable")
    parser.add_argument("--no-smoke-test", action="store_true", help="do not start the result")
    args = parser.parse_args()

    triple = args.target or host_triple()
    if args.universal and sys.platform != "darwin":
        raise SystemExit("--universal only makes sense on macOS")

    work, built = build(args.universal)
    try:
        if not args.no_smoke_test:
            ok, detail = smoke_test([str(built)])
            print(f"smoke test: {detail}")
            if not ok:
                raise SystemExit("The compiled daemon does not work, not installing it")

        OUT_DIR.mkdir(parents=True, exist_ok=True)
        for name in sidecar_names(triple, args.universal):
            shutil.copy2(built, OUT_DIR / name)
            print(f"installed {OUT_DIR / name}")
    finally:
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
