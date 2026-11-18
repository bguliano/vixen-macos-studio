# Vixen Studio and the vanilla port

Vixen Studio is a redesigned edition of [the unofficial macOS port](https://github.com/bguliano/vixen-macos-port) ("vanilla").
Vanilla stays close to Windows Vixen; Studio is free to change the interface, the workflows and, where needed, the engine.

Both read and write the same Vixen 3 show folders, so a show can move between vanilla, Studio and Windows Vixen.
Studio has its own bundle identifier (`io.github.bguliano.vixen-macos-studio`) and app name, so it installs beside vanilla
and keeps its own preferences.

## Branches and remotes

| Remote | Repository | Branch |
|---|---|---|
| `origin` | `bguliano/vixen-macos-studio` | `studio` (default) |
| `upstream` | `bguliano/vixen-macos-port` | `macos-port` (fetch only; pushing is disabled) |

Studio carries vanilla's full history, so vanilla changes arrive as ordinary merges.

## Taking vanilla changes

```sh
scripts/studio_sync.sh            # fetch upstream, merge upstream/macos-port, check every Studio marker survived
scripts/studio_sync.sh --check    # only check the markers against the last recorded inventory
```

The script refuses to finish a merge that would drop or alter a Studio-marked block. On a conflict it stops and lists the
conflicted files, marking which ones contain Studio blocks. Resolve those by keeping the Studio side of each marked block,
then run `scripts/studio_sync.sh --continue`.

A GitHub Action (`.github/workflows/sync-vanilla.yml`) opens a "Sync from vanilla" pull request every Monday when vanilla
has new commits, running the same marker check.

## Marking Studio changes

Anything that differs from vanilla **outside the Studio-only files** is marked, so a merge can never silently overwrite it.

- A changed block, in any language:

  ```rust
  // STUDIO-BEGIN render-progress: report per-effect render progress to the toolbar
  ...
  // STUDIO-END render-progress
  ```

  Use `#` for shell, Python and TOML, and `<!-- STUDIO-BEGIN name: ... -->` in XML and plist. Names are unique,
  lowercase and hyphenated; the text after the colon says why the block differs.
- A one-line change: a trailing `// STUDIO: why` (or `# STUDIO: why`).
- Every marked block is listed in [`studio/markers.lock`](studio/markers.lock), with a hash of its contents. The sync
  script regenerates the list and fails if a block vanished or changed during a merge. After an intentional edit to a
  marked block, run `scripts/studio_sync.sh --record`.

Studio-only files need no markers, because vanilla never touches them: everything under `studio/`, `app/Sources/Studio*`,
`crates/studio-*`, `STUDIO.md`, and any file whose name starts with `studio_` or `Studio`. Prefer putting new code there, so merges stay clean.

A few vanilla files are rewritten wholesale for Studio (`README.md`, `docs/UI_GUIDE.md`). `.gitattributes` gives them the
`ours` merge driver, so vanilla's edits to them never overwrite Studio's. Read vanilla's change to such a file in the
merge's diff and carry over anything that still applies.

## Engine changes

Studio may change the Rust engine (`crates/`). Each change is marked as above, and listed in the table below with the
reason. If a change would also help vanilla, note it here so it can be offered upstream.

| Marker | Crate | Why | Offer to vanilla? |
|---|---|---|---|
| _(none yet)_ | | | |
