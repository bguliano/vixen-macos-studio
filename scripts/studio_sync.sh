#!/bin/bash
# Takes vanilla's changes into Vixen Studio without losing Studio's own. See STUDIO.md.
#
#   scripts/studio_sync.sh              fetch upstream, merge upstream/macos-port, verify every Studio marker
#   scripts/studio_sync.sh --continue   after resolving conflicts: verify markers, then commit the merge
#   scripts/studio_sync.sh --check      verify markers against studio/markers.lock
#   scripts/studio_sync.sh --record     rewrite studio/markers.lock after an intentional edit to a marked block
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
BRANCH="${VANILLA_BRANCH:-macos-port}"

case "${1:-}" in
    --check) exec python3 scripts/studio_markers.py check ;;
    --record) exec python3 scripts/studio_markers.py record ;;
    --continue)
        if git diff --name-only --diff-filter=U | grep -q .; then
            echo "Unresolved conflicts remain:"; git diff --name-only --diff-filter=U; exit 1
        fi
        python3 scripts/studio_markers.py check
        git commit --no-edit
        exit 0 ;;
    "") ;;
    *) echo "usage: scripts/studio_sync.sh [--continue|--check|--record]" >&2; exit 2 ;;
esac

if ! git diff --quiet || ! git diff --cached --quiet; then
    echo "Commit or stash your changes first." >&2; exit 1
fi
python3 scripts/studio_markers.py check

git remote get-url upstream >/dev/null 2>&1 || {
    git remote add upstream https://github.com/bguliano/vixen-macos-port.git
    git remote set-url --push upstream DISABLE_PUSH_TO_VANILLA
}
git config merge.ours.driver true  # .gitattributes: Studio-owned files keep Studio's version
git fetch upstream "$BRANCH"
if git merge-base --is-ancestor "upstream/$BRANCH" HEAD; then
    echo "Already up to date with vanilla."; exit 0
fi

echo "==> Merging $(git rev-list --count HEAD..upstream/"$BRANCH") vanilla commit(s)"
if ! git merge --no-ff --no-commit "upstream/$BRANCH" -m "Sync from vanilla ($(git rev-parse --short upstream/"$BRANCH"))"; then
    echo
    echo "Conflicts. Files holding Studio markers are flagged; keep the Studio side of each marked block:"
    for f in $(git diff --name-only --diff-filter=U); do
        if grep -q "STUDIO" "$f" 2>/dev/null; then echo "  [STUDIO] $f"; else echo "           $f"; fi
    done
    echo "Then run: scripts/studio_sync.sh --continue"
    exit 1
fi
git submodule update --init --depth 1 upstream/Vixen >/dev/null 2>&1 || true
if ! python3 scripts/studio_markers.py check; then
    echo "The merge altered Studio-marked code. It is staged but not committed; fix it, then run --continue." >&2
    exit 1
fi
git commit --no-edit
echo "Merged vanilla into $(git branch --show-current)."
