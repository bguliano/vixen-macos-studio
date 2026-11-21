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
| `sequence-inspector-undo`, `sequence-inspector-inverse` | vixen-session | Undo the sequence inspector's mark collection visibility switches, restoring each collection's own lines and bar (`MarksChanged` restores marks only). | No: Studio inspector. |
| `sequence-audio-undo`, `sequence-audio-inverse`, `sequence-audio-record`, `sequence-audio-record-failed`, `sequence-audio-record-added`, `sequence-audio-speed`, `sequence-audio-waveform` (and one-line markers in `sequencer/commands.rs`, `SequencerModel.swift`, `SequencerCommands.swift`) | vixen-session, vixen-ffi, app | Add, Change and Remove Audio are one undo step each, restoring the media entries, timing source, decoded clip and the length the change set; the waveform and speed controls follow the session's song after Undo or an agent's `sequence_audio` (`studio_sequence_audio.rs`, `StudioSequenceAudio.swift`; B3). | Yes: upstream's audio association has no undo. |
| Quiet marks (one-line markers in `timeline/{model,marks,session_link}.rs` and `Painters.swift`), `quiet-mark-ticks`, `snapping-mark-lines`, `snapping-mark-lines-apply` | vixen-session, vixen-ffi | Marks are ruler and waveform ticks; the grid's tiles draw only the selected marks' collections, and an overlay adds the rest while an effect drag snaps (`studio_mark_ticks.rs`). The waveform repaints when marks or their selection change. | No: Studio's timeline design. |
| Row height (one-line markers in `timeline/{model,session_link}.rs`, `SequencerModel.swift`), `row-height-mode`, `row-height-record`, `row-height-menu` | vixen-session, vixen-ffi, app | View ▸ Row Height, Compact or Regular: the timeline follows a `DefaultRowHeight` that `sequence_row_height` gave the document (`studio_row_height.rs`). | No: Studio's View menu. |
| Recently Used (one-line markers in `timeline/host.rs`, `sequencer/state.rs`, `SequencerModel.swift`) | vixen-session, app | The Effects library's Recently Used records the effects the user places in the sequencer window (drop, Add Effect, drawn drag); favorites and recents are the user's (`studio_effects_library.rs`). | No: Studio's Effects library. |
| Agent flash (one-line markers in `sequence.rs`, `Surfaces.swift`, `SequencerModel.swift`), `agent-flash-record`, `agent-flash` | vixen-session, vixen-ffi, app | Effects an agent adds or changes flash once on an open sequence's grid; the document notes them as agent edits go on the Undo stack (`studio_agent_flash.rs`). | No: Studio's agent UI. |
| `marks-panel-undo`, `marks-panel-inverse`, `marks-panel-params`, `marks-panel-undo-ffi`, `marks-live-offset`, `marks-header-label`, `beats-undo` (one-line labels in `vixen-ffi` `marks.rs`) | vixen-session, vixen-ffi | The Mark Collections panel's edits, imports and Beats and Bars detection are live and one step each on the shared Undo, restoring the collection list and mark-reading effects' parameters (`studio_marks_panel.rs`); Offset Time is a live popover whose changes are one step (R40). | Possibly: upstream's panel has no undo. |
| LipSync image maps (one-line markers in `vixen-session` and `vixen-ffi` `lib.rs`) | vixen-session, vixen-ffi | Edit Image Maps' changes are live and one step each on the shared history, keeping the picture folders each step changed so Undo restores them with the library (`studio_lipsync.rs`; B7). | Possibly: upstream's sheet saves on close and has no undo. |
| `lyric-map-word` (and one-line markers in `LyricConverterView.swift`, `SequencerLipSync.swift`, `vixen-session` and `vixen-ffi` `lib.rs`) | vixen-session, vixen-ffi, app | The Text Conversion Failure sheet confirms with Skip Word and Map Word; a mapping is one step on the shared history that keeps the user dictionary file's previous bytes, so Undo puts the file back, and the Lyric Converter window mirrors it into its Edit ▸ Undo (`studio_phoneme_mappings.rs`; B22). | Possibly: upstream's mapping has no undo. |
| `face-mapping-live`, `face-mapping-visit`, `face-mapping-check` (and one-line markers in `FaceMappingNodeSelectSheet.swift`, `vixen-session` and `vixen-ffi` `lib.rs`) | vixen-session, vixen-ffi, app | Edit Element Face Mapping's node select sheet confirms with Set Up Faces… and closes with Done; each Face Setup visit is one step on the shared history ("Edit Face Mapping") that renders LipSync effects again without rebuilding the output, mirrored into the sheet's Edit ▸ Undo (`studio_face_mapping.rs`; B23). | Possibly: upstream's Face Setup has no undo. |
| `render-queue-dedupe`, `render-queue-skip`, `render-result-dedupe` | vixen-session | Edits arriving faster than rendering (agent tool calls, scripted bulk edits) re-queued every pending effect each time, and the duplicate results each added schedule entries: quadratic memory growth (200 agent edits on a 554-pixel group: 20M entries, 4.2 GB, vs 221,600 entries once reopened). Each effect generation is now queued and applied once. | Yes: the same race exists for fast UI edits. |
| Preview color conversion visibility | vixen-session | Reuse real color handling in offscreen inspection frames. | Possibly. |
| `preview-video-deps` (Cargo.toml, marked Cargo.lock lines) | vixen-session | AVFoundation bindings for the MCP preview_video H.264/AAC writer; the code itself is Studio-only (`studio_video.rs`). | No: agent review tool. |
| `preview-revision` | vixen-session | The Previews pane's preview list edits (`studio_previews.rs`) move the display revision without rebuilding the output graph, so they work with Live Output on. | No. |
| `schedule-replace-items` (scheduler `run.rs`), scheduler attach (one-line marker, vixen-ffi) | vixen-session, vixen-ffi | The Schedule pane's show and schedule edits (`studio_schedule.rs`) and their Undo hand the running scheduler its new schedule list at once, keeping runs whose schedule didn't change. | No: Studio's shared history. |
| `timeline-selected-targets` | vixen-ffi | The timeline's selected rows (or active row) as display elements, the targets of Edit ▸ Place Effect on Marks…. | No: Studio command sheet. |
| `preview-design-undo` | vixen-ffi | The preview designer's write-back is one step on the shared undo history ("Edit Preview Layout"), skipped when the layout is unchanged. | No: Studio's shared history. |
| `designer-live-undo`, `designer-live-dialogs`, `designer-shape-export`, `designer-live-offset`, `designer-live-fixture-link`, `designer-live-linking` (and one-line labels in `session.rs`, `preview.rs`, `DesignerModel.swift`, `DesignerDialogs.swift`) | vixen-preview, vixen-ffi, vixen-session, app | The designer's Linked Elements, Link Element and Location Offset sheets are live and Resize Background is one step, each on the designer's own undo stack, without OK/Cancel; a run of arrow-key nudges is one Move step (`studio_design_undo.rs`, `studio_preview_designer.rs`; R41, B12). | Possibly: upstream's designer doesn't undo these. |
| Dialogs live: `dialog-prompt-verb`, `dialog-text-verb`, `e131-live`, `e131-live-hooks`, `coarse-fine-always-live`, `layer-chroma-live`, `layer-luma-live`, `layer-mask-live`, `grid-live`, `state-mapper-live`, `state-mapper-live-hooks`, `fixture-functions-live`, `fixture-functions-done`, `fixture-property-live`, `fixture-property-done` (and one-line markers across `VixenDialogs`) | app, vixen-session, vixen-ffi | The property, filter, controller and layer setup sheets apply each change as it's made and close with Done; a draft's changes are steps on the sheet's Edit ▸ Undo, and Display Setup publishes a sheet's whole visit as one named step ("Configure Controller"; `StudioLiveSheets.swift`, `publish_setup`'s label). Value prompts confirm with verbs (B9). | Possibly: upstream's dialogs commit on OK only. |
| Curve editor live: `curve-editor-restore`, `curve-editor-restore-test`, `curve-editor-restore-ffi`, `curve-editor-live`, `curve-editor-live-model`, `curve-editor-live-view`, `curve-editor-live-done`, `curve-editor-live-hooks`, `curve-editor-live-filter`, `curve-editor-live-helper`, `curve-editor-live-wizard` (and one-line markers in `CurveEditorDialog.swift`, `RowEditors.swift`, `FixtureWizardPages.swift`) | vixen-session, vixen-ffi, app | The curve editor hands each finished change to its owner at once (an effect's curve row, the Dimming Curve filter, a library entry, the Dimming Curve helper, the fixture wizard) and closes with Done; each change is a step on the sheet's Edit ▸ Undo, which `CurveEditSession::restore` puts back, and an effect's whole visit is one step on the sequence's Undo (`StudioCurveEditorLive.swift`, `StudioCurveEditorVisit.swift`; B24). | Possibly: upstream's curve editor commits on OK only. |
| Polygon Editor live (one-line markers in `vixen-session` `tools/polygon.rs`, `vixen-ffi` `tools/polygon.rs`, `PolygonEditor.swift`, `PolygonCanvas.swift`), `polygon-live`, `polygon-live-done` | vixen-session, vixen-ffi, app | The Polygon Editor writes each change to the edited Morph effects at once and closes with Done, recording everything done in the window as one "Polygons" undo step, or none when the shapes end where they started (`tools/polygon/studio_live.rs`; B8). | Possibly: upstream's editor commits on OK only. |
| `timeline-access`, `designer-access`, `graph-access` (and one-line labels in `vixen-session` `lib.rs`) | vixen-ffi, vixen-preview, vixen-session | VoiceOver labels and keyboard moves for the timeline grid, the patching graph and the preview designer, built in Rust and shared with `sequence_effects`' label (`studio_access.rs`, `studio_timeline_access.rs`, `studio_graph_access.rs`; R43). | Possibly: upstream's canvases have no accessibility. |
| `controllers_changed` visibility (one-line marker) | vixen-session | The controller inspector's network edits (`studio_controller_network.rs`) mark controllers changed as Display Setup's own edits do. | No. |
| `authoring-modules` and Studio module registrations | vixen-session | Register Studio-only command/schema/diagnostic implementations without changing vanilla UI behavior. | No. |
| `mcp-app-start`, `shared-sequence-wrapper`, `reuse-open-sequence`, Studio FFI/dependency hooks, Display Setup session visibility | vixen-ffi | Serve the running core and share documents/undo with agents; expose callbacks without view changes. | Generic shared document reuse may help vanilla. |

See `studio/MCP-INTEGRATION.md` for the complete marked-file handoff and validation evidence.
