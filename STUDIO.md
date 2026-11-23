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
`crates/studio-*`, `docs/plugins/` (the plugin developer documentation), `STUDIO.md`, and any file whose name starts with `studio_` or `Studio`. Prefer putting new code there, so merges stay clean.

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
| Inspector and sidebar drawers: `inspector-drawer-menu`, `sidebar-drawer-menu` (and one-line markers in `SequencerModel.swift`, `SequencerContent.swift`) | app | The toolbar's Inspector button and View ▸ Hide/Show Inspector (⌥⌘I) collapse the whole right-hand drawer without closing its tabs, and the Effects button and View ▸ Hide/Show Sidebar (⌃⌘S) the whole left-hand one; new windows open as the last one was left, and showing a panel on a collapsed side opens it again (`StudioInspectorDrawer.swift`; #69, B50). | No: Studio's toolbar. |
| Recently Used (one-line markers in `timeline/host.rs`, `sequencer/state.rs`, `SequencerModel.swift`) | vixen-session, app | The Effects library's Recently Used records the effects the user places in the sequencer window (drop, Add Effect, drawn drag); favorites and recents are the user's (`studio_effects_library.rs`). | No: Studio's Effects library. |
| Agent flash (one-line markers in `sequence.rs`, `Surfaces.swift`, `SequencerModel.swift`), `agent-flash-record`, `agent-flash` | vixen-session, vixen-ffi, app | Effects an agent adds or changes flash once on an open sequence's grid; the document notes them as agent edits go on the Undo stack (`studio_agent_flash.rs`). | No: Studio's agent UI. |
| `marks-panel-undo`, `marks-panel-inverse`, `marks-panel-params`, `marks-panel-undo-ffi`, `marks-live-offset`, `marks-header-label`, `beats-undo` (one-line labels in `vixen-ffi` `marks.rs`) | vixen-session, vixen-ffi | The Mark Collections panel's edits, imports and Beats and Bars detection are live and one step each on the shared Undo, restoring the collection list and mark-reading effects' parameters (`studio_marks_panel.rs`); Offset Time is a live popover whose changes are one step (R40). | Possibly: upstream's panel has no undo. |
| Timing from Sequence (one-line markers in `vixen-ffi` `marks.rs` and `MarksTestScene.swift`) | vixen-ffi, app | The Mark Collections panel's Import ▸ Timing from Sequence… reads another profile's `.tim` and offers its effects as mark collections (`studio_marks_from_sequence.rs` in vixen-session and vixen-ffi; #61). | Possibly: upstream imports only mark files. |
| LipSync image maps (one-line markers in `vixen-session` and `vixen-ffi` `lib.rs`) | vixen-session, vixen-ffi | Edit Image Maps' changes are live and one step each on the shared history, keeping the picture folders each step changed so Undo restores them with the library (`studio_lipsync.rs`; B7). | Possibly: upstream's sheet saves on close and has no undo. |
| `lyric-map-word` (and one-line markers in `LyricConverterView.swift`, `SequencerLipSync.swift`, `vixen-session` and `vixen-ffi` `lib.rs`) | vixen-session, vixen-ffi, app | The Text Conversion Failure sheet confirms with Skip Word and Map Word; a mapping is one step on the shared history that keeps the user dictionary file's previous bytes, so Undo puts the file back, and the Lyric Converter window mirrors it into its Edit ▸ Undo (`studio_phoneme_mappings.rs`; B22). | Possibly: upstream's mapping has no undo. |
| `face-mapping-live`, `face-mapping-visit`, `face-mapping-check` (and one-line markers in `FaceMappingNodeSelectSheet.swift`, `vixen-session` and `vixen-ffi` `lib.rs`) | vixen-session, vixen-ffi, app | Edit Element Face Mapping's node select sheet confirms with Set Up Faces… and closes with Done; each Face Setup visit is one step on the shared history ("Edit Face Mapping") that renders LipSync effects again without rebuilding the output, mirrored into the sheet's Edit ▸ Undo (`studio_face_mapping.rs`; B23). | Possibly: upstream's Face Setup has no undo. |
| `render-queue-dedupe`, `render-queue-skip`, `render-result-dedupe` | vixen-session | Edits arriving faster than rendering (agent tool calls, scripted bulk edits) re-queued every pending effect each time, and the duplicate results each added schedule entries: quadratic memory growth (200 agent edits on a 554-pixel group: 20M entries, 4.2 GB, vs 221,600 entries once reopened). Each effect generation is now queued and applied once. | Yes: the same race exists for fast UI edits. |
| Bars Step on Marks: `bars-step-marks` (generated table), `bars-step-visibility`, `bars-step-marks-select`, `bars-step-marks-render`, `bars-step-marks-position`, `bars-step-marks-helper`, `bars-step-marks-flat` (and one-line markers in `bars.rs`, `fx-pixel-motion` and `vixen-effects` `lib.rs`); `studio-members-skip`, `studio-members-absent` (and a one-line marker in `vixen-io` `lib.rs`) | fx-pixel-motion, vixen-effects, vixen-io | Flat Bars can step one notch per mark of a mark collection (StepToMarks, MarkCollectionId, StepsPerCycle) instead of moving continuously (`studio_mark_steps.rs`, `studio_bars_steps.rs`; #67), or every StepEveryMs ms without marks (B55). Studio-only effect members stay out of the file at their defaults and read their defaults when absent (`studio_effect_members.rs`), so untouched effects save exactly as before; Windows Vixen ignores them and vanilla keeps them as unknown members. | No: Windows Vixen has no such option. |
| Spiral Step on Marks: `spiral-step-marks` (generated table), `spiral-step-visibility`, `spiral-step-marks-select`, `spiral-step-marks-render`, `spiral-step-marks-position`, `spiral-step-marks-helper` (and one-line markers in `spiral.rs` and `fx-pixel-simple` `lib.rs`) | fx-pixel-simple, vixen-io | Spiral can turn one notch per mark of a mark collection (StepToMarks, MarkCollectionId, StepsPerCycle) instead of rotating continuously, or every StepEveryMs ms without marks (B55), reusing `studio_mark_steps.rs` (`studio_spiral_steps.rs`; B51). The members are Studio-only effect members (`studio_effect_members.rs`), left out of the file at their defaults. | No: Windows Vixen has no such option. |
| Preview color conversion visibility | vixen-session | Reuse real color handling in offscreen inspection frames. | Possibly. |
| `preview-video-deps` (Cargo.toml, marked Cargo.lock lines) | vixen-session | AVFoundation bindings for the MCP preview_video H.264/AAC writer; the code itself is Studio-only (`studio_video.rs`). | No: agent review tool. |
| `preview-revision` | vixen-session | The Previews pane's preview list edits (`studio_previews.rs`) move the display revision without rebuilding the output graph, so they work with Live Output on. | No. |
| `schedule-replace-items` (scheduler `run.rs`), scheduler attach (one-line marker, vixen-ffi) | vixen-session, vixen-ffi | The Schedule pane's show and schedule edits (`studio_schedule.rs`) and their Undo hand the running scheduler its new schedule list at once, keeping runs whose schedule didn't change. | No: Studio's shared history. |
| Effect Gallery: `effect-gallery` (and one-line markers in `vixen-ffi` `lib.rs` and `SequencerSheets.swift`) | vixen-ffi, app | Edit ▸ Effect Gallery… shows looping thumbnails of effects on the selected row's prop from the effect_gallery code, and places the one clicked at the cursor (`studio_effect_gallery.rs` in vixen-session and vixen-ffi, `StudioEffectGallerySheet.swift`; #62). | No: Studio sequencer sheet. |
| `timeline-selected-targets` | vixen-ffi | The timeline's selected rows (or active row) as display elements, the targets of Edit ▸ Place Effect on Marks…. | No: Studio command sheet. |
| `preview-design-undo` | vixen-ffi | The preview designer's write-back is one step on the shared undo history ("Edit Preview Layout"), skipped when the layout is unchanged. | No: Studio's shared history. |
| `designer-live-undo`, `designer-live-dialogs`, `designer-shape-export`, `designer-live-offset`, `designer-live-fixture-link`, `designer-live-linking` (and one-line labels in `session.rs`, `preview.rs`, `DesignerModel.swift`, `DesignerDialogs.swift`) | vixen-preview, vixen-ffi, vixen-session, app | The designer's Linked Elements, Link Element and Location Offset sheets are live and Resize Background is one step, each on the designer's own undo stack, without OK/Cancel; a run of arrow-key nudges is one Move step (`studio_design_undo.rs`, `studio_preview_designer.rs`; R41, B12). | Possibly: upstream's designer doesn't undo these. |
| Dialogs live: `dialog-prompt-verb`, `dialog-text-verb`, `e131-live`, `e131-live-hooks`, `coarse-fine-always-live`, `layer-chroma-live`, `layer-luma-live`, `layer-mask-live`, `grid-live`, `state-mapper-live`, `state-mapper-live-hooks`, `fixture-functions-live`, `fixture-functions-done`, `fixture-property-live`, `fixture-property-done` (and one-line markers across `VixenDialogs`) | app, vixen-session, vixen-ffi | The property, filter, controller and layer setup sheets apply each change as it's made and close with Done; a draft's changes are steps on the sheet's Edit ▸ Undo, and Display Setup publishes a sheet's whole visit as one named step ("Configure Controller"; `StudioLiveSheets.swift`, `publish_setup`'s label). Value prompts confirm with verbs (B9). | Possibly: upstream's dialogs commit on OK only. |
| Curve editor live: `curve-editor-restore`, `curve-editor-restore-test`, `curve-editor-restore-ffi`, `curve-editor-live`, `curve-editor-live-model`, `curve-editor-live-view`, `curve-editor-live-done`, `curve-editor-live-hooks`, `curve-editor-live-filter`, `curve-editor-live-helper`, `curve-editor-live-wizard` (and one-line markers in `CurveEditorDialog.swift`, `RowEditors.swift`, `FixtureWizardPages.swift`) | vixen-session, vixen-ffi, app | The curve editor hands each finished change to its owner at once (an effect's curve row, the Dimming Curve filter, a library entry, the Dimming Curve helper, the fixture wizard) and closes with Done; each change is a step on the sheet's Edit ▸ Undo, which `CurveEditSession::restore` puts back, and an effect's whole visit is one step on the sequence's Undo (`StudioCurveEditorLive.swift`, `StudioCurveEditorVisit.swift`; B24). | Possibly: upstream's curve editor commits on OK only. |
| Gradient editor live: `gradient-editor-restore`, `gradient-editor-restore-test`, `gradient-editor-restore-ffi`, `gradient-editor-live`, `gradient-editor-live-model`, `gradient-editor-live-done`, `gradient-editor-live-hooks` (and one-line markers in `GradientEditorDialog.swift`, `RowEditors.swift`) | vixen-session, vixen-ffi, app | The Color Gradient Editor hands each finished change to its owner at once (an effect's gradient row, a library entry) and closes with Done; each change is a step on the sheet's Edit ▸ Undo, which `GradientEditSession::restore` puts back, and an effect's whole visit is one step on the sequence's Undo (`StudioGradientEditorLive.swift`, `StudioGradientEditorVisit.swift`; B27). | Possibly: upstream's gradient editor commits on OK only. |
| Polygon Editor live (one-line markers in `vixen-session` `tools/polygon.rs`, `vixen-ffi` `tools/polygon.rs`, `PolygonEditor.swift`, `PolygonCanvas.swift`), `polygon-live`, `polygon-live-done` | vixen-session, vixen-ffi, app | The Polygon Editor writes each change to the edited Morph effects at once and closes with Done, recording everything done in the window as one "Polygons" undo step, or none when the shapes end where they started (`tools/polygon/studio_live.rs`; B8). | Possibly: upstream's editor commits on OK only. |
| Single bulbs: `designer-single-lights`, `designer-single-lights-draw`, `designer-add-menu` (and one-line markers in `DesignerWindow.swift` and `vixen-session` `lib.rs`) | vixen-preview, vixen-session, app | The Single Light tool with several elements or a group selected places one bulb per light, linked and named after it, as one Undo step; Edit ▸ Add names the tool and adds Bulbs for Selected Elements; Display ▸ Elements ▸ Add ▸ Single Bulbs… and MCP `element_single_bulbs` make, colour, patch and show several at once (`studio_single_lights.rs`, `studio_single_bulbs.rs`, `StudioSingleBulbsSheet.swift`; B60). | Possibly: upstream's tool links a bulb to one selected light only. |
| `timeline-access`, `designer-access`, `graph-access` (and one-line labels in `vixen-session` `lib.rs`) | vixen-ffi, vixen-preview, vixen-session | VoiceOver labels and keyboard moves for the timeline grid, the patching graph and the preview designer, built in Rust and shared with `sequence_effects`' label (`studio_access.rs`, `studio_timeline_access.rs`, `studio_graph_access.rs`; R43). | Possibly: upstream's canvases have no accessibility. |
| Automatic updates: `sparkle-package` (and one-line markers in `Package.swift`), `sparkle-settings` (Info.plist), `sparkle-package-framework`, `sparkle-bundle` (`build_app.sh`), `sparkle-release-usage`, `sparkle-release-stamp`, `sparkle-release-sign` (`package_release.sh`), one-line markers in `vixen-session` and `vixen-ffi` `lib.rs`, `AdministrationCommands.swift`, `AdministrationScene.swift` | vixen-session, vixen-ffi, app, release | Sparkle 2 (pinned 2.10.1): Vixen Studio ▸ Check for Updates…, Settings ▸ General ▸ Updates, MCP app_update_status and app_update_check; vanilla's Help ▸ Check for Updates (official Windows Vixen releases) is renamed Help ▸ Official Vixen Releases… so the two never share a name (B53); the updater waits while a sequence plays or Live Output is on (`studio_updates.rs`, `StudioUpdater.swift`, `scripts/studio_sparkle.sh`; releases get a feed only once the signing key exists, `ops/RELEASES.md`; B49). | No: Studio's own releases. |
| `controllers_changed` visibility (one-line marker) | vixen-session | The controller inspector's network edits (`studio_controller_network.rs`) mark controllers changed as Display Setup's own edits do. | No. |
| `authoring-modules` and Studio module registrations | vixen-session | Register Studio-only command/schema/diagnostic implementations without changing vanilla UI behavior. | No. |
| Plugin host (one-line markers in the workspace `Cargo.toml`, `vixen-ffi` `Cargo.toml` and `lib.rs`; `plugins-lock-dependency` in `Cargo.lock`; `plugins-shutdown` and a one-line marker in `VixenApp.swift`; a one-line marker in `SetupModel.swift`) | vixen-ffi, app | Link the Studio-only plugin host (`crates/studio-plugins`) and expose it to Settings ▸ Plugins, the Plugins menu and the controller inspector (`studio_plugins.rs`; design in [`studio/PLUGINS-DESIGN.md`](studio/PLUGINS-DESIGN.md), developer docs in [`docs/plugins/`](docs/plugins/README.md)); stop plugins when Studio quits; read controller types each time so plugin types installed later can be added. | No: Studio-only feature. |
| Plugin SDK (a one-line marker in the workspace `Cargo.toml`; `plugin-sdk-lock-entry` in `Cargo.lock`) | workspace | The Rust plugin SDK (`crates/studio-plugin-sdk`, package `vixen-plugin-sdk`) is a workspace member so `cargo test` builds and checks the Rust example and template; the app never links it. | No: Studio-only feature. |
| Plugin controllers: `plugins-module-counters`, `plugins-add-controller`, `plugins-store-entries`, `plugins-store-config` (and one-line markers in `vixen-output` `modules/mod.rs`, `setup/controllers.rs`, `boot.rs`, `vixen-ffi` `admin.rs`) | vixen-output, vixen-session, vixen-ffi | Plugin controller types load, save, run and report like built-in ones through one provider hook (`modules/studio_external.rs`, `studio_plugin_controllers.rs`); their module data is kept whole so it survives saves without the plugin. | No: Studio-only feature. |
| `mcp-app-start`, `shared-sequence-wrapper`, `reuse-open-sequence`, Studio FFI/dependency hooks, Display Setup session visibility | vixen-ffi | Serve the running core and share documents/undo with agents; expose callbacks without view changes. | Generic shared document reuse may help vanilla. |

See `studio/MCP-INTEGRATION.md` for the complete marked-file handoff and validation evidence.
