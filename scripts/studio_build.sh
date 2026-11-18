#!/bin/bash
# Runs scripts/build_app.sh behind a machine-wide lock, so parallel worktrees never build at the same time.
# Arguments and environment pass straight through (VIXEN_BUILD_ID, VIXEN_AREAS, debug|release).
#
# Set STUDIO_ADMISSION to a command that must succeed before building (for example a free-memory check).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LOCK="${TMPDIR:-/tmp}/vixen-studio-build.lock"
waited=0
until mkdir "$LOCK" 2>/dev/null; do
    if [ -f "$LOCK/pid" ] && ! kill -0 "$(cat "$LOCK/pid")" 2>/dev/null; then
        rm -rf "$LOCK"; continue   # stale lock from a build that died
    fi
    [ $((waited % 30)) -eq 0 ] && echo "Waiting for another Studio build ($(cat "$LOCK/owner" 2>/dev/null || echo unknown))..."
    sleep 5; waited=$((waited + 5))
done
echo $$ > "$LOCK/pid"
echo "$ROOT" > "$LOCK/owner"
trap 'rm -rf "$LOCK"' EXIT
if [ -n "${STUDIO_ADMISSION:-}" ]; then
    bash -c "$STUDIO_ADMISSION"
fi
nice -n 10 "$ROOT/scripts/build_app.sh" "$@"
