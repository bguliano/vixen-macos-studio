#!/usr/bin/env python3
"""Check concurrent native profile ownership and normal Shutdown using a new empty profile."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    assert not out.exists(), "Use a fresh output directory"
    out.mkdir(parents=True)
    profile = out / "profile"
    profile.mkdir()
    executable = args.app.resolve() / "Contents/MacOS/Vixen"
    env = {key: value for key, value in os.environ.items() if not key.startswith(("VIXEN_", "DYLD_"))}
    env.update(VIXEN_DATA=str(profile), VIXEN_MUTE="1", VIXEN_UI_TEST_TRACE="1")
    processes, logs = [], []

    def launch(name, steps):
        folder = out / name
        folder.mkdir()
        script = folder / "steps.json"
        script.write_text(json.dumps({"steps": steps}, indent=2))
        log = (folder / "process.log").open("w")
        logs.append(log)
        proc = subprocess.Popen([str(executable)], env=env | {
            "VIXEN_UI_TEST": str(script), "VIXEN_UI_TEST_OUT": str(folder)},
            stdout=log, stderr=subprocess.STDOUT)
        processes.append(proc)
        return proc, folder

    def wait_for(path, proc, seconds=10):
        deadline = time.monotonic() + seconds
        while not path.exists() and proc.poll() is None and time.monotonic() < deadline:
            time.sleep(.05)
        assert path.exists(), f"Missing {path}; process exit={proc.poll()}"

    opening = [{"action": "window", "id": "administration"}, {"action": "wait", "seconds": 1},
               {"action": "dump", "name": "administration"}]
    shutdown = {"action": "menu", "path": ["Vixen Studio", "Quit Vixen Studio"]}  # STUDIO: Shutdown and Exit is the app menu's Quit (B69)
    # Termination is asynchronous; do not let the harness's direct finish/exit
    # race the production shutdown task.
    shutdown_steps = [shutdown, {"action": "wait", "seconds": 10}]
    try:
        owner, owner_out = launch("owner", opening + [{"action": "wait", "seconds": 12}] + shutdown_steps)
        wait_for(owner_out / "administration.txt", owner)
        lock = profile / ".lock"
        original_lock = lock.read_bytes()
        info = profile / "logs/Application.Info.log"
        assert info.read_text().count("Vixen System successfully started.") == 1
        second, second_out = launch("contender", opening + [
            {"action": "snapshot", "name": "locked"}, {"action": "wait", "seconds": .2},
            {"action": "click", "label": "OK"}, {"action": "wait", "seconds": 3}])
        wait_for(second_out / "administration.txt", second)
        text = (second_out / "administration.txt").read_text()
        assert "Profile is already in use or unable to the lock the profile." in text, text
        assert second.wait(timeout=10) == 0
        assert owner.poll() is None, "Contender stopped the owning app"
        assert lock.read_bytes() == original_lock, "Contender removed or rewrote the owner's lock"
        assert owner.wait(timeout=20) == 0
        assert not lock.exists(), "Normal Shutdown retained the lock"
        reopened, reopened_out = launch("reopened", opening + [
            {"action": "snapshot", "name": "reopened"}] + shutdown_steps)
        wait_for(reopened_out / "administration.txt", reopened)
        text = (reopened_out / "administration.txt").read_text()
        assert "Profile is already in use" not in text, text
        assert info.read_text().count("Vixen System successfully started.") == 2
        assert reopened.wait(timeout=15) == 0 and not lock.exists()
        for folder in (owner_out, second_out, reopened_out):
            assert " failed:" not in (folder / "process.log").read_text()
        report = {"passed": True, "sameProfileConcurrentProcesses": 2, "launches": 3,
                  "contenderRejected": True, "ownerLockPreserved": True,
                  "normalShutdownRemovedLock": True, "reopenSucceeded": True,
                  "muted": True, "freshProfile": str(profile)}
        (out / "result.json").write_text(json.dumps(report, indent=2))
        print(json.dumps(report))
    finally:
        for process in processes:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=3)
        for log in logs:
            log.close()


if __name__ == "__main__":
    main()
