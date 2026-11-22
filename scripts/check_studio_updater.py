#!/usr/bin/env python3
"""End-to-end check of Vixen Studio's automatic updates (Sparkle 2, B49), entirely on this Mac.

Usage: check_studio_updater.py [path/to/Vixen.app]      (default build/Vixen.app, a debug build)

With a throwaway EdDSA key (made with openssl here, never the real one) and an appcast served from 127.0.0.1, it
makes an "old" build 100 and a "new" build 101 of the given app under a test bundle id, then checks, through the UI test
harness and the `studio.updater` hook:

1. a build without a feed has no updater (status enabled:false; no Check for Updates… in the app menu);
2. the old build shows Check for Updates… under About and the Updates section in Settings, and an information check
   (what `app_update_check` runs) finds build 101 without downloading anything;
3. with automatic updates on, the old build downloads and verifies 101; Install and Relaunch while Live Output is on
   offers to wait for the show (the relaunch is held), Restart Now installs it over itself and relaunches: the
   relaunched app (driven through LSEnvironment) reports build 101;
4. a scheduled check that finds an update while Live Output is on keeps it as a gentle reminder, with no window, and
   shows it once Live Output goes off;
5. an update whose signature doesn't match the key is refused: lastError says so and the installed app stays build 100.

Everything lives under target/updater-e2e and is removed afterwards, along with the test bundle id's preferences.
Output stays muted (VIXEN_MUTE) and the profile is a copy of target/show-copy's configuration.
"""
import base64
import http.server
import json
import os
import plistlib
import shutil
import subprocess
import sys
import threading
import time
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
TEST_ID = "io.github.bguliano.vixen-macos-studio.updatetest"
WORK = REPO / "target/updater-e2e"


def run(*args, **kw):
    return subprocess.run(args, check=True, text=True, capture_output=True, **kw).stdout


def test_key(folder: Path):
    """A throwaway EdDSA key: the private seed (Sparkle's key-file format) and the public key, both base64."""
    pem = folder / "test.pem"
    run("openssl", "genpkey", "-algorithm", "ed25519", "-out", str(pem))
    seed = subprocess.run(["openssl", "pkey", "-in", str(pem), "-outform", "DER"], check=True, capture_output=True).stdout[-32:]
    public = subprocess.run(["openssl", "pkey", "-in", str(pem), "-pubout", "-outform", "DER"], check=True, capture_output=True).stdout[-32:]
    key_file = folder / "test-key.txt"
    key_file.write_text(base64.b64encode(seed).decode())
    pem.unlink()
    return key_file, base64.b64encode(public).decode()


def make_build(source: Path, dest: Path, build: int, feed: str, public_key: str, env: dict | None):
    shutil.rmtree(dest, ignore_errors=True)
    dest.parent.mkdir(parents=True, exist_ok=True)
    run("/usr/bin/ditto", str(source), str(dest))
    plist_path = dest / "Contents/Info.plist"
    info = plistlib.loads(plist_path.read_bytes())
    info["CFBundleIdentifier"] = TEST_ID
    info["CFBundleVersion"] = str(build)
    if feed:
        info["SUFeedURL"] = feed
        info["SUPublicEDKey"] = public_key
    if env:
        info["LSEnvironment"] = env
    plist_path.write_bytes(plistlib.dumps(info))
    run("/usr/bin/codesign", "--force", "--sign", "-", "--timestamp=none", str(dest))


def harness(app: Path, script: dict, out: Path, data: Path, timeout=240, quits_itself=False):
    out.mkdir(parents=True, exist_ok=True)
    (out / "script.json").write_text(json.dumps(script))
    env = dict(os.environ, VIXEN_UI_TEST=str(out / "script.json"), VIXEN_UI_TEST_OUT=str(out), VIXEN_DATA=str(data), VIXEN_MUTE="1")
    subprocess.run([str(app / "Contents/MacOS/Vixen")], env=env, timeout=timeout, capture_output=True)
    if quits_itself:
        return []
    return json.loads((out / "result.json").read_text())


def hook_status(steps, index=-1):
    details = [s.get("detail", "") for s in steps if s["action"] == "hook"]
    return json.loads(details[index])


def defaults(*args):
    subprocess.run(["defaults", *args], capture_output=True)


def forget_test_app():
    """The test bundle id's preferences and Sparkle's download cache, so every run starts clean."""
    defaults("delete", TEST_ID)
    for folder in ("Library/Caches", "Library/HTTPStorages", "Library/Saved Application State"):
        for path in (Path.home() / folder).glob(TEST_ID + "*"):
            shutil.rmtree(path, ignore_errors=True)


def wait_for(path: Path, seconds: float):
    deadline = time.time() + seconds
    while time.time() < deadline and not path.exists():
        time.sleep(1)
    return path.exists()


def main():
    source = Path(sys.argv[1]) if len(sys.argv) > 1 else REPO / "build/Vixen.app"
    bin_dir = Path(run(str(REPO / "scripts/studio_sparkle.sh")).strip()) / "bin"
    shutil.rmtree(WORK, ignore_errors=True)
    www, apps = WORK / "www", WORK / "Apps"
    www.mkdir(parents=True)
    data = WORK / "profile"
    for part in ("SystemData", "Module Data Files"):
        shutil.copytree(REPO / "target/show-copy" / part, data / part, symlinks=False)
    key_file, public_key = test_key(WORK)

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), lambda *a: http.server.SimpleHTTPRequestHandler(*a, directory=str(www)))
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = f"http://127.0.0.1:{server.server_port}"
    feed = f"{base}/appcast.xml"
    forget_test_app()
    installed = apps / "Vixen Studio.app"
    try:
        # 1. No feed: no updater.
        make_build(source, installed, 100, "", "", None)
        steps = harness(installed, {"steps": [
            {"action": "window", "id": "administration"}, {"action": "wait", "seconds": 3},
            {"action": "menus", "name": "menus"},
            {"action": "hook", "name": "studio.updater", "path": ["status"]}, {"action": "quit"}]}, WORK / "run-nofeed", data)
        assert all(s["ok"] for s in steps), steps
        assert hook_status(steps)["enabled"] is False, steps
        nofeed_menus = (WORK / "run-nofeed/menus.txt").read_text()
        assert "Check for Updates" not in nofeed_menus, nofeed_menus[:800]
        assert "Official Vixen Releases…" in nofeed_menus, nofeed_menus[:800]
        print("ok  no feed: updater off, no menu item")

        # The new build, signed with the test key, and an appcast offering it.
        relaunch_out = WORK / "run-relaunched"
        relaunch_script = WORK / "relaunched.json"
        relaunch_script.write_text(json.dumps({"steps": [
            {"action": "wait", "seconds": 3}, {"action": "hook", "name": "studio.updater", "path": ["status"]}, {"action": "quit"}]}))
        relaunch_env = {"VIXEN_UI_TEST": str(relaunch_script), "VIXEN_UI_TEST_OUT": str(relaunch_out), "VIXEN_DATA": str(data), "VIXEN_MUTE": "1"}
        staging = WORK / "staging/Vixen Studio.app"
        make_build(source, staging, 101, feed, public_key, relaunch_env)
        zip_path = www / "Vixen-Studio-101.zip"
        run("/usr/bin/ditto", "-c", "-k", "--keepParent", str(staging), str(zip_path))
        signature = run(str(bin_dir / "sign_update"), "-p", "--ed-key-file", str(key_file), str(zip_path)).strip()
        notes = WORK / "notes.md"
        notes.write_text("### Changes\n\n- Test update (b49)\n")

        def appcast(build, zip_file, sig):
            target = www / "appcast.xml"
            target.unlink(missing_ok=True)
            run(sys.executable, str(REPO / "scripts/studio_appcast.py"), str(target),
                "--title", f"Vixen Studio test {build}", "--version", str(build), "--short-version", f"3.13.4 test {build}",
                "--url", f"https://placeholder/{zip_file.name}", "--length", str(zip_file.stat().st_size), "--signature", sig,
                "--notes-url", "https://placeholder/notes", "--notes-file", str(notes), "--min-system", "26.0")
            target.write_text(target.read_text().replace("https://placeholder", base))

        appcast(101, zip_path, signature)

        # 2. Menu item, Settings section, and an information check that finds 101 without downloading.
        make_build(source, installed, 100, feed, public_key, None)
        defaults("write", TEST_ID, "SUEnableAutomaticChecks", "-bool", "false")
        steps = harness(installed, {"steps": [
            {"action": "window", "id": "administration"}, {"action": "wait", "seconds": 3},
            {"action": "menus", "name": "menus"},
            {"action": "menu", "path": ["Vixen Studio", "Settings…"]}, {"action": "wait", "seconds": 1.5},
            {"action": "snapshot", "name": "settings"},
            {"action": "hook", "name": "studio.updater", "path": ["information"]}, {"action": "wait", "seconds": 4},
            {"action": "hook", "name": "studio.updater", "path": ["status"]}, {"action": "quit"}]}, WORK / "run-info", data)
        assert all(s["ok"] for s in steps), steps
        menus = (WORK / "run-info/menus.txt").read_text()
        assert menus.count("Check for Updates") == 1, menus[:800]
        # The grouped form's rows aren't in the harness's accessibility dump: look at settings.png (KEEP_UPDATER_E2E=1).
        assert (WORK / "run-info/settings.png").stat().st_size > 10_000
        status = hook_status(steps)
        assert status["enabled"] and status["available"]["build"] == "101", status
        assert status["readyToInstallOnQuit"] is False and status["lastError"] is None, status
        print("ok  menu item, Settings ▸ Updates, information check found build 101:", status["available"]["version"])

        # 3. Automatic download, install and relaunch.
        defaults("write", TEST_ID, "SUEnableAutomaticChecks", "-bool", "true")
        defaults("write", TEST_ID, "SUAutomaticallyUpdate", "-bool", "true")
        defaults("write", TEST_ID, "SULastCheckTime", "-date", "2026-01-01 00:00:00 +0000")
        # Downloaded in the background, then the user's Install and Relaunch: Live Output is on (the harness's default
        # too), so Studio offers to wait for the show, and Restart Now goes ahead.
        steps = harness(installed, {"steps": [
            {"action": "window", "id": "administration"}, {"action": "wait", "seconds": 2},
            {"action": "hook", "name": "studio.updater", "path": ["waitReady", "120"]},
            {"action": "hook", "name": "studio.updater", "path": ["check"]}, {"action": "wait", "seconds": 4},
            {"action": "dump", "name": "sparkle"},
            {"action": "click", "label": "Install and Relaunch"}, {"action": "wait", "seconds": 4},
            {"action": "dump", "name": "alert"},
            {"action": "hook", "name": "studio.updater", "path": ["status"]},
            {"action": "click", "label": "Restart Now"},
            {"action": "wait", "seconds": 30}]}, WORK / "run-install", data, timeout=300, quits_itself=True)
        install_notes = (WORK / "run-install/notes.txt").read_text().splitlines()
        statuses = [json.loads(line.removeprefix("updater status: ")) for line in install_notes if line.startswith("updater status: ")]
        ready, waiting = statuses[0], statuses[2]
        assert "updater: installing build 101" in install_notes and "updater: relaunching" in install_notes, install_notes
        assert ready["readyToInstallOnQuit"] and ready["lastError"] is None, ready
        assert "Install and Relaunch" in (WORK / "run-install/sparkle.txt").read_text()
        alert = (WORK / "run-install/alert.txt").read_text()
        assert "restart to finish updating when the show stops" in alert and "Live Output is on" in alert, alert
        assert waiting["showRunning"] and waiting["waitingForShowToStop"] and waiting["currentBuild"] == "100", waiting
        assert wait_for(relaunch_out / "result.json", 90), "the updated app didn't relaunch"
        relaunched = hook_status(json.loads((relaunch_out / "result.json").read_text()))
        assert relaunched["currentBuild"] == "101", relaunched
        info = plistlib.loads((installed / "Contents/Info.plist").read_bytes())
        assert info["CFBundleVersion"] == "101", info["CFBundleVersion"]
        print("ok  downloaded, verified, installed and relaunched: build", relaunched["currentBuild"])

        # 4. A scheduled check finding an update while Live Output is on keeps it as a gentle reminder (no window), and
        #    shows it once Live Output goes off.
        make_build(source, installed, 100, feed, public_key, None)
        appcast(101, zip_path, signature)
        forget_test_app()
        defaults("write", TEST_ID, "SUEnableAutomaticChecks", "-bool", "true")
        defaults("write", TEST_ID, "SUAutomaticallyUpdate", "-bool", "false")
        defaults("write", TEST_ID, "SULastCheckTime", "-date", "2026-01-01 00:00:00 +0000")
        steps = harness(installed, {"steps": [
            {"action": "window", "id": "administration"}, {"action": "wait", "seconds": 8},
            {"action": "windows"},
            {"action": "hook", "name": "studio.updater", "path": ["status"]},
            {"action": "menu", "path": ["System", "Execution Engine", "Stop"]}, {"action": "wait", "seconds": 20},
            {"action": "windows"},
            {"action": "hook", "name": "studio.updater", "path": ["status"]}, {"action": "quit"}]}, WORK / "run-deferred", data)
        assert all(s["ok"] for s in steps), steps
        windows = [s.get("detail", "") for s in steps if s["action"] == "windows"]
        before, after = hook_status(steps, 0), hook_status(steps, 1)
        def update_window_showing(listing):
            return any(w.startswith("Software Update ") and not w.endswith("(hidden)") for w in listing.split(" | "))
        assert before["showRunning"] and before["deferred"] and not update_window_showing(windows[0]), (before, windows[0])
        assert not after["showRunning"] and not after["deferred"] and update_window_showing(windows[1]), (after, windows[1])
        print("ok  scheduled update waited while Live Output was on, shown once it went off")

        # 5. A bad signature is refused.
        make_build(source, installed, 100, feed, public_key, None)
        make_build(source, staging, 102, feed, public_key, None)
        bad_zip = www / "Vixen-Studio-102.zip"
        run("/usr/bin/ditto", "-c", "-k", "--keepParent", str(staging), str(bad_zip))
        good = base64.b64decode(run(str(bin_dir / "sign_update"), "-p", "--ed-key-file", str(key_file), str(bad_zip)).strip())
        forged = base64.b64encode(bytes([good[0] ^ 0xFF]) + good[1:]).decode()
        appcast(102, bad_zip, forged)
        forget_test_app()
        defaults("write", TEST_ID, "SUEnableAutomaticChecks", "-bool", "true")
        defaults("write", TEST_ID, "SUAutomaticallyUpdate", "-bool", "true")
        defaults("write", TEST_ID, "SULastCheckTime", "-date", "2026-01-01 00:00:00 +0000")
        steps = harness(installed, {"steps": [
            {"action": "window", "id": "administration"}, {"action": "wait", "seconds": 2},
            {"action": "hook", "name": "studio.updater", "path": ["waitReady", "120"]}, {"action": "quit"}]}, WORK / "run-bad", data, timeout=300)
        status = hook_status(steps)
        assert not status["readyToInstallOnQuit"] and status["lastError"], status
        time.sleep(3)
        info = plistlib.loads((installed / "Contents/Info.plist").read_bytes())
        assert info["CFBundleVersion"] == "100", info["CFBundleVersion"]
        print("ok  bad signature refused:", status["lastError"])
    finally:
        server.shutdown()
        forget_test_app()
        subprocess.run(["pkill", "-f", str(apps)], capture_output=True)
        if os.environ.get("KEEP_UPDATER_E2E") != "1":
            shutil.rmtree(WORK, ignore_errors=True)


if __name__ == "__main__":
    main()
