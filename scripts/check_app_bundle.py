#!/usr/bin/env python3
"""Read-only macOS bundle audit. No launch, signing, network or user-data writes.

Checks bundle metadata, resource byte parity, code signature, architecture/deployment
loads, contained symlinks and every Mach-O dependency/rpath. This proves local
self-containment; it does not establish notarization or compatibility on another OS.
"""
from __future__ import annotations
import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import subprocess
import sys
import tomllib

REPO = Path(__file__).resolve().parents[1]
MACHO = {b"\xcf\xfa\xed\xfe", b"\xfe\xed\xfa\xcf", b"\xce\xfa\xed\xfe", b"\xfe\xed\xfa\xce", b"\xca\xfe\xba\xbe", b"\xbe\xba\xfe\xca", b"\xca\xfe\xba\xbf", b"\xbf\xba\xfe\xca"}


def inside(path: Path, root: Path) -> bool:
    return path.resolve().is_relative_to(root.resolve())


def system_path(path: str) -> bool:
    normalized = os.path.normpath(path)
    return normalized.startswith("/System/Library/") or normalized.startswith("/usr/lib/")


def version(value: str) -> tuple[int, ...]:
    parts = tuple(int(part) for part in value.split("."))
    return parts + (0,) * (3 - len(parts))


def audit(app: Path, resources: Path, required_arch: str) -> dict:
    app = app.resolve()
    report = {"bundle": str(app), "ok": False, "errors": [], "metadata": {}, "resources": {}, "macho": [],
              "limits": ["No Developer ID or notarization claim", "No launch or other-Mac compatibility claim"]}
    errors = report["errors"]

    def run(*args: str) -> str:
        result = subprocess.run(args, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
        if result.returncode:
            errors.append(f"{' '.join(args[:2])}: {result.stderr.strip() or result.stdout.strip()}")
        return result.stdout + (result.stderr if args[0] == "/usr/bin/codesign" else "")

    info_path = app / "Contents/Info.plist"
    try:
        with info_path.open("rb") as file:
            info = plistlib.load(file)
    except (OSError, plistlib.InvalidFileException, ValueError) as error:
        errors.append(f"Info.plist: {error}")
        return report
    for key in ["CFBundleIdentifier", "CFBundleExecutable", "CFBundleShortVersionString", "CFBundleVersion", "LSMinimumSystemVersion"]:
        if not isinstance(info.get(key), str) or not info[key].strip():
            errors.append(f"Missing string metadata: {key}")
    if errors:
        return report
    report["metadata"] = {key: info[key] for key in ["CFBundleIdentifier", "CFBundleShortVersionString", "CFBundleVersion", "LSMinimumSystemVersion"]}
    if info.get("CFBundlePackageType") != "APPL":
        errors.append("CFBundlePackageType must be APPL")
    executable = app / "Contents/MacOS" / info["CFBundleExecutable"]
    if not inside(executable, app) or not executable.is_file() or not os.access(executable, os.X_OK):
        errors.append("CFBundleExecutable is missing, outside the bundle, or not executable")
    launch_helper = app / "Contents/Helpers/vixen-launch-helper"
    if not inside(launch_helper, app) or not launch_helper.is_file() or not os.access(launch_helper, os.X_OK):
        errors.append("Native launch helper is missing, outside the bundle, or not executable")
    for tool in ["ffmpeg", "ffprobe"]:
        path = app / "Contents/Helpers" / tool
        if not inside(path, app) or not path.is_file() or not os.access(path, os.X_OK):
            errors.append(f"Bundled media tool {tool} is missing, outside the bundle, or not executable")
    try:
        media_manifest = json.loads((app / "Contents/Resources/MediaTools/manifest.json").read_text())
        for tool in ["ffmpeg", "ffprobe"]:
            entry = next(value for value in media_manifest["files"] if value["name"] == tool)
            path = app / "Contents/Helpers" / tool
            if inside(path, app) and path.is_file() and hashlib.sha256(path.read_bytes()).hexdigest() != entry["sha256"]:
                errors.append(f"Bundled media tool {tool} differs from signed build provenance")
    except (OSError, ValueError, KeyError, TypeError, StopIteration) as error:
        errors.append(f"Bundled media tool provenance: {error}")
    try:
        minimum = version(info["LSMinimumSystemVersion"])
    except ValueError:
        errors.append("Invalid LSMinimumSystemVersion")
        return report
    with (REPO / "Cargo.toml").open("rb") as file:
        project_version = tomllib.load(file)["workspace"]["package"]["version"]
    if info["CFBundleShortVersionString"] != project_version:
        errors.append(f"Bundle version differs from workspace version {project_version}")
    icon = info.get("CFBundleIconFile", "")
    icon_path = app / "Contents/Resources" / icon
    if not icon or not inside(icon_path, app) or not any(path.is_file() for path in [icon_path, icon_path.with_suffix(".icns")]):
        errors.append("Declared application icon is missing")
    for path in app.rglob("*"):
        if path.is_symlink() and (not inside(path, app) or not path.exists()):
            errors.append(f"External or broken bundle symlink: {path.relative_to(app)}")

    bundled = app / "Contents/Resources"
    source_files = sorted(path for path in resources.rglob("*") if path.is_file())
    if not source_files:
        errors.append(f"No source resources found: {resources}")
    total_bytes = 0
    for source in source_files:
        relative = source.relative_to(resources)
        target = bundled / relative
        if not inside(target, app):
            errors.append(f"Resource resolves outside the bundle: {relative}")
        elif not target.is_file():
            errors.append(f"Missing resource: {relative}")
        elif hashlib.sha256(source.read_bytes()).digest() != hashlib.sha256(target.read_bytes()).digest():
            errors.append(f"Resource bytes differ: {relative}")
        total_bytes += source.stat().st_size
    report["resources"] = {"source": str(resources.resolve()), "files": len(source_files), "bytes": total_bytes}

    run("/usr/bin/codesign", "--verify", "--deep", "--strict", str(app))
    signature = run("/usr/bin/codesign", "-dvv", str(app))
    report["signature"] = {"adhoc": "Signature=adhoc" in signature,
                           "team": next((line.split("=", 1)[1] for line in signature.splitlines() if line.startswith("TeamIdentifier=")), None)}
    native_files = []
    for path in app.rglob("*"):
        if path.is_file() and not path.is_symlink():
            with path.open("rb") as file:
                if file.read(4) in MACHO:
                    native_files.append(path)
    if executable not in native_files:
        errors.append("Main executable is not a Mach-O image")
    for native in native_files:
        architectures = run("/usr/bin/lipo", "-archs", str(native)).strip().split()
        if required_arch not in architectures:
            errors.append(f"{native.relative_to(app)} lacks required architecture {required_arch}")
        commands = run("/usr/bin/otool", "-l", str(native))
        rpaths = re.findall(r"cmd LC_RPATH\s+cmdsize \d+\s+path (.*?) \(offset", commands)
        builds = re.findall(r"cmd LC_BUILD_VERSION\s+cmdsize \d+\s+platform (\d+)\s+minos ([\d.]+)", commands)
        if not builds:
            errors.append(f"{native.relative_to(app)} has no LC_BUILD_VERSION")
        for platform, minos in builds:
            if platform != "1" or version(minos) > minimum:
                errors.append(f"{native.relative_to(app)} platform/minimum {platform}/{minos} contradicts bundle minimum {info['LSMinimumSystemVersion']}")

        def expand(value: str) -> str:
            return value.replace("@loader_path", str(native.parent), 1).replace("@executable_path", str(executable.parent), 1)

        for rpath in rpaths:
            expanded = expand(rpath)
            if not expanded.startswith("/") or not (system_path(expanded) or inside(Path(expanded), app)):
                errors.append(f"Nonportable rpath in {native.relative_to(app)}: {rpath}")
        dependencies = []
        own_ids = {line for line in run("/usr/bin/otool", "-D", str(native)).splitlines() if not line.endswith(":")}  # STUDIO: a framework's own install name (Sparkle, B49)
        for line in run("/usr/bin/otool", "-L", str(native)).splitlines():
            if not line.startswith("\t"):
                continue
            dependency = line.strip().split(" (compatibility version", 1)[0]
            if dependency in own_ids:  # STUDIO: otool -L lists a library's own install name first; it isn't a dependency
                continue
            dependencies.append(dependency)
            candidates = [expand(dependency)]
            if dependency.startswith("@rpath/"):
                candidates = [expand(path) + "/" + dependency.removeprefix("@rpath/") for path in rpaths]
            if not any(system_path(candidate) or
                       (candidate.startswith("/") and inside(Path(candidate), app) and Path(candidate).is_file())
                       for candidate in candidates):
                errors.append(f"Unresolved/nonportable dependency in {native.relative_to(app)}: {dependency}")
        report["macho"].append({"path": str(native.relative_to(app)), "architectures": architectures,
                                "build_versions": [{"platform": p, "minimum": m} for p, m in builds], "rpaths": rpaths, "dependencies": dependencies})
    report["ok"] = not errors
    return report


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("app", type=Path)
    parser.add_argument("--resources", type=Path, default=REPO / "app/Resources")
    parser.add_argument("--require-arch", default="arm64")
    parser.add_argument("--out", type=Path)
    args = parser.parse_args()
    report = audit(args.app, args.resources, args.require_arch)
    output = json.dumps(report, indent=2) + "\n"
    if args.out:
        args.out.write_text(output)
    print(output, end="")
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    sys.exit(main())
