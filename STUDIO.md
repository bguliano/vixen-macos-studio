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

## Replaced views

Where Studio redesigned a window, it usually added its own view and left vanilla's file in place, unused or partly
bypassed, so merges stay clean. Those pairs are listed in [`studio/replaced-views.txt`](studio/replaced-views.txt).
After a merge, `scripts/studio_sync.sh` lists any of them vanilla changed, so the change can be ported by hand.

## Engine changes

Studio may change the Rust engine (`crates/`). Each change is marked as above, and listed in the table below with the
reason. If a change would also help vanilla, note it here so it can be offered upstream.

| Marker | Crate | Why | Offer to vanilla? |
|---|---|---|---|
| `mcp-profile-lock` | vixen-io | Recognize the lowercase Studio MCP helper as an active profile lock owner. | No: Studio helper-specific. |
| `exclusive-save-temp` and owned-temp cleanup | vixen-io | Prevent predictable temporary-file symlinks from redirecting atomic saves; never remove a temp this call did not create. | Yes. |
| `display-materialize`, AppCore/boot hooks, module-only Dirty flag | vixen-session | Shared live transactions, lifecycle shutdown gate and module-only preview/show edits; the confirmed-cleanup hook lets the live Display pane run OK's cleanup when it is left. | Generic materialization/dirty support may help vanilla. |
| `sequence-settings-undo`, `sequence-settings-inverse` | vixen-session | Undo document length and layer configuration on the native stack. | Yes. |
| `render-queue-dedupe`, `render-queue-skip`, `render-result-dedupe` | vixen-session | Edits arriving faster than rendering (agent tool calls, scripted bulk edits) re-queued every pending effect each time, and the duplicate results each added schedule entries: quadratic memory growth (200 agent edits on a 554-pixel group: 20M entries, 4.2 GB, vs 221,600 entries once reopened). Each effect generation is now queued and applied once. | Yes: the same race exists for fast UI edits. |
| Preview color conversion visibility | vixen-session | Reuse real color handling in offscreen inspection frames. | Possibly. |
| `preview-video-deps` (Cargo.toml, marked Cargo.lock lines) | vixen-session | AVFoundation bindings for the MCP preview_video H.264/AAC writer; the code itself is Studio-only (`studio_video.rs`). | No: agent review tool. |
| `preview-revision` | vixen-session | The Previews pane's preview list edits (`studio_previews.rs`) move the display revision without rebuilding the output graph, so they work with Live Output on. | No. |
| `schedule-replace-items` (scheduler `run.rs`), scheduler attach (one-line marker, vixen-ffi) | vixen-session, vixen-ffi | The Schedule pane's show and schedule edits (`studio_schedule.rs`) and their Undo hand the running scheduler its new schedule list at once, keeping runs whose schedule didn't change. | No: Studio's shared history. |
| `timeline-selected-targets` | vixen-ffi | The timeline's selected rows (or active row) as display elements, the targets of Edit ▸ Place Effect on Marks…. | No: Studio command sheet. |
| `preview-design-undo` | vixen-ffi | The preview designer's write-back is one step on the shared undo history ("Edit Preview Layout"), skipped when the layout is unchanged. | No: Studio's shared history. |
| `controllers_changed` visibility (one-line marker) | vixen-session | The controller inspector's network edits (`studio_controller_network.rs`) mark controllers changed as Display Setup's own edits do. | No. |
| `authoring-modules` and Studio module registrations | vixen-session | Register Studio-only command/schema/diagnostic implementations without changing vanilla UI behavior. | No. |
| `mcp-app-start`, `shared-sequence-wrapper`, `reuse-open-sequence`, Studio FFI/dependency hooks, Display Setup session visibility | vixen-ffi | Serve the running core and share documents/undo with agents; expose callbacks without view changes. | Generic shared document reuse may help vanilla. |

See `studio/MCP-INTEGRATION.md` for the complete marked-file handoff and validation evidence.
