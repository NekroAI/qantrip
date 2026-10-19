"""Isolated KWin/Xwayland + GApplication smoke test; never uses the user's session.

Run after cargo build --manifest-path apps/linux/Cargo.toml --features debug-hooks:
    uv run tests/linux/hotkeys_smoke.py
Requires kwin_wayland, Xwayland, dbus-daemon, gdbus, and the Rust toolchain.
This verifies virtual display/IPC behavior, not real desktop Portal authorization.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
BINARY = ROOT / "apps/linux/target/debug/qantrip"
APP_ID = "ai.nekro.qantrip"
BUS_CONFIG = """<busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen>
<auth>EXTERNAL</auth><policy context="default"><allow send_destination="*"/>
<allow receive_sender="*"/><allow own="*"/></policy></busconfig>"""


def call(*arguments: str) -> str:
    return subprocess.check_output([str(BINARY), *arguments], text=True, timeout=8)


def owner_pid() -> int | None:
    result = subprocess.run(
        ["gdbus", "call", "--session", "--dest", "org.freedesktop.DBus",
         "--object-path", "/org/freedesktop/DBus",
         "--method", "org.freedesktop.DBus.GetConnectionUnixProcessID", APP_ID],
        capture_output=True, text=True, timeout=5,
    )
    if result.returncode:
        return None
    return int(result.stdout.removeprefix("(uint32 ").removesuffix(",)\n"))


def wait_owner() -> int:
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        pid = owner_pid()
        if pid is not None:
            return pid
        time.sleep(0.05)
    raise AssertionError("GApplication did not acquire its bus name")


def status() -> dict:
    return json.loads(call("debug", "status"))


def inside() -> None:
    # KWin passes its own WAYLAND_DISPLAY and DISPLAY to this session child.
    os.environ["GDK_BACKEND"] = "wayland"
    config = Path(os.environ["XDG_CONFIG_HOME"]) / "qantrip"
    config.mkdir(parents=True)
    (config / "settings.json").write_text(
        json.dumps({"onboardingDone": True, "interfaceLanguage": "en"})
    )
    primary = subprocess.Popen([str(BINARY), "--background"])
    try:
        assert wait_owner() == primary.pid
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            initial = status()
            if initial["hotkey_state"].startswith("Failed"):
                break
            time.sleep(0.05)
        assert initial["hotkey_mode"] == "Portal", initial
        assert "Unsupported" in initial["hotkey_state"], initial
        assert not initial["panel_visible"]
        for command, visible in [
            ("toggle", True), ("toggle", False), ("show", True), ("show", True),
        ]:
            call(command)
            assert status()["panel_visible"] == visible, (command, status())
            assert owner_pid() == primary.pid
        call("debug", "key", "esc")
        assert not status()["panel_visible"]
        # Each forwarded toggle executes once: eight actions must leave it hidden.
        commands = [subprocess.Popen([str(BINARY), "toggle"]) for _ in range(8)]
        for command in commands:
            assert command.wait(timeout=8) == 0
        assert not status()["panel_visible"]
        assert owner_pid() == primary.pid
        call("settings")
        call("debug", "shot", "settings", str(Path(os.environ["QANTRIP_SMOKE_OUTPUT"]) / "settings.png"))
        call("quit")
        assert primary.wait(timeout=5) == 0
    finally:
        if primary.poll() is None:
            primary.terminate()
            primary.wait(timeout=5)

    # Cold concurrent starts must select one owner and dispatch both commands.
    processes = [subprocess.Popen([str(BINARY), "show"]) for _ in range(2)]
    try:
        assert wait_owner() in [process.pid for process in processes]
        assert status()["panel_visible"]
        call("quit")
        for process in processes:
            assert process.wait(timeout=8) == 0
    finally:
        for process in processes:
            if process.poll() is None:
                process.terminate()
                process.wait(timeout=5)

    # Even when GTK uses Xwayland, native Wayland coverage cannot be claimed.
    xenv = dict(os.environ, GDK_BACKEND="x11")
    process = subprocess.Popen([str(BINARY), "--background"], env=xenv)
    try:
        assert wait_owner() == process.pid
        assert status()["hotkey_mode"] == "Portal"
        call("quit")
        assert process.wait(timeout=5) == 0
    finally:
        if process.poll() is None:
            process.terminate()
            process.wait(timeout=5)

    missing_bus = dict(os.environ, DBUS_SESSION_BUS_ADDRESS=f"unix:path={os.environ['XDG_RUNTIME_DIR']}/missing-bus")
    failed = subprocess.run([str(BINARY), "toggle"], env=missing_bus, capture_output=True, timeout=8)
    assert failed.returncode != 0, "A missing session bus must not start a non-unique GUI"

    subprocess.run(
        ["cargo", "test", "--locked", "--manifest-path", str(ROOT / "apps/linux/Cargo.toml"),
         "--features", "debug-hooks", "x11_server_regression", "--", "--ignored", "--test-threads=1"],
        env=dict(os.environ, QANTRIP_TEST_X11="1"), check=True, timeout=60,
    )
    print("PASS: virtual Wayland/Xwayland selection, CLI forwarding, toggle parity, "
          "concurrent startup, missing-bus refusal, cleanup, and isolated X11 grabs/controlled events/conflicts.")
    (Path(os.environ["QANTRIP_SMOKE_OUTPUT"]) / "passed").write_text("passed\n")


def main() -> None:
    if "--inside" in sys.argv:
        inside()
        return
    if not BINARY.exists():
        raise SystemExit("Build the debug-hooks binary first.")
    output = Path(tempfile.mkdtemp(prefix="qantrip-hotkeys-smoke-"))
    with tempfile.TemporaryDirectory(prefix="qantrip-hotkeys-session-") as temporary:
        base = Path(temporary)
        runtime = base / "runtime"
        runtime.mkdir(mode=0o700)
        bus_config = base / "bus.conf"
        bus_config.write_text(BUS_CONFIG)
        with (output / "session.log").open("w") as log:
            bus = subprocess.Popen(
                ["dbus-daemon", f"--config-file={bus_config}", "--nofork", "--print-address=1"],
                stdout=subprocess.PIPE, stderr=log, text=True,
            )
            assert bus.stdout is not None
            address = bus.stdout.readline().strip()
            env = {
                key: value for key, value in os.environ.items()
                if key not in {"DISPLAY", "WAYLAND_DISPLAY", "WAYLAND_SOCKET", "XAUTHORITY",
                               "DESKTOP_STARTUP_ID", "XDG_ACTIVATION_TOKEN", "DBUS_SESSION_BUS_ADDRESS"}
            }
            env.update(
                DBUS_SESSION_BUS_ADDRESS=address, XDG_RUNTIME_DIR=str(runtime),
                XDG_CONFIG_HOME=str(base / "config"), XDG_DATA_HOME=str(base / "data"),
                XDG_STATE_HOME=str(base / "state"), XDG_CACHE_HOME=str(base / "cache"),
                XDG_SESSION_TYPE="wayland", XDG_CURRENT_DESKTOP="QantripTest",
                QT_QPA_PLATFORM="offscreen", QT_QPA_PLATFORMTHEME="", QT_STYLE_OVERRIDE="Fusion",
                KWIN_COMPOSE="Q", LIBGL_ALWAYS_SOFTWARE="1",
                GTK_A11Y="none", QANTRIP_DEBUG_HOOKS="1", QANTRIP_SMOKE_OUTPUT=str(output),
            )
            launcher = base / "session.sh"
            launcher.write_text(
                "#!/bin/sh\nexec " + shlex.join([sys.executable, str(Path(__file__).resolve()), "--inside"]) + "\n"
            )
            launcher.chmod(0o700)
            try:
                subprocess.run(
                    ["kwin_wayland", "--virtual", "--xwayland", "--no-lockscreen", "--no-kactivities",
                     "--width", "1280", "--height", "800", "--exit-with-session", str(launcher)],
                    env=env, stdout=log, stderr=log, timeout=90, check=True,
                )
                if not (output / "passed").exists():
                    raise AssertionError(f"Smoke test failed; inspect {output / 'session.log'}")
            finally:
                bus.terminate()
                bus.wait(timeout=5)
    print(f"Smoke test passed. Artifacts: {output}")


if __name__ == "__main__":
    main()
