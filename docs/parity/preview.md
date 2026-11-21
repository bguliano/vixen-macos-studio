# Preview parity

> **Handover, pass 2 (2026-11-09):** the Elements tree is editable in the designer (F82, F85, F86) through `ElementTreeEditor.make` in VixenKit, which VixenSetup fills in with `TreeEditRunner`. Build with `VIXEN_AREAS=preview,admin,setup,dialogs` to get it and the wizards. The element template wizards now run from the designer (F29, F30a, F81) through `ElementTemplateRunner.run` in VixenKit, which VixenSetup fills in. No `WindowID` was needed: they are sheets on the designer's window. The custom prop reader (`load_prop`) is the specialized editors worker's; check `docs/parity/specialized-editors.md` before starting F7, F40-F44 or F57.

> **Handover, pass 3 (2026-11-11):** authored custom-prop faces and states now import with remapped element IDs. Moving heads now render in live camera views and as clipped designer thumbnails, with profile constraints, wheel/shutter/dimmer setup, independent strobe/travel clock and native legends. Bottom-mounted tilt uses the owner-confirmed OpenGL +180° convention in every fixture view. See the verification section below. Remaining partial rows name verification or integration work explicitly.

Status of every entry in `docs/spec/preview.md`. This table is how passes hand over to each other.

**Status.** **done**: built and looked at, with where it lives. **partial**: built, with what's missing named. **todo**: not built yet. **elsewhere**: another area owns the rest. **omitted**: not built, with why.

**Paths.** `P/` is `app/Sources/VixenPreview/`. `FFI` is `crates/vixen-ffi/src/preview.rs`. `session/` is `crates/vixen-session/src/`. `crate/` is `crates/vixen-preview/src/`. `render/` is `crates/vixen-render/src/`.

## Features

| F | Feature | Status | Where | Checked, and what's missing or different |
|---|---|---|---|---|
| F1 | Open the Preview Configuration window | done | P/PreviewScene.swift, P/DesignerWindow.swift `DesignerWindowRoot` | Opened from Setup Previews' Configure Preview in the real app, which stopped the running preview first. 1194x601 by default, minimum 1020x590. Its own window, not a sheet, so it doesn't block other windows the way upstream's modal dialog does; Setup Previews still waits for `previewDesignerClosed` |
| F2 | Load sequence | done | FFI `open_preview_designer`, `load_background`; P/DesignerModel.swift `open` | Harness and real window on both real previews. The background is decoded off the main thread after the window shows, once (hazards 17, 19) |
| F3 | Prop repair on open (`VerifyPreviewShapeLocations`) | done | crate `setup_open`; FFI `take_verify_message` | Rust. The alert goes through `alert`, not seen on real data because nothing there needs repair |
| F4 | Close the window | done | P/DesignerModel.swift `shouldClose`, `confirmClose`, `DesignerWindowDelegate` | Alerts "Invalid Element Links" with "Close Anyway" and "Keep Editing", then F20, then the commit off the main thread, then the close. Generated invalid-link profile: native close opens warning, Keep Editing retains the designer and selects invalid shapes, Close Anyway closes it (18 steps; `target/preview-partials/invalid-out`) |
| F5 | Window position and size persist | done | P/DesignerModel.swift `restoreBounds`, `DesignerWindowDelegate.reportBounds` | November12 passes24+10 native assertions with UI_TEST inactive: actual model/Metal open restores saved outer frame, automatic AppKit move and supplied end-resize callbacks save four top-left/y-down fields, fresh Core reopen matches, offscreen saved bounds fall back, and a1100×680 right-corner-only frame restores without AppKit clamping. Exact unchanged copied Preview module with optimized production dependencies; unshown raw NSWindow host, not full SwiftUI/Finder or physical-drag evidence. See `docs/review/preview-bounds-native.md`. |
| F6 | File > Save (Ctrl+S) | done | P/DesignerCommands.swift `.newItem` insertion; P/DesignerModel.swift `save`; SequencerRegistry settled focus and guarded menu actions | Dedicated and combined Save pass. The combined84-step foreground run on candidate9c verifies actual key/main ownership, live ModuleStore writes before close, unchanged sequence hashes on Preview saves, unchanged Preview hashes on sequence saves, and isolated Undo/Redo. `target/preview-foreground-newitem-acceptance/result.json`; local CG-backed AppKit events, not physical keys. |
| F7 | File > Import Prop | done | `DesignerModel.importProp`, `session/preview_import`, `vixen-prop::load_prop` | Legacy LiteDB and authored packages load locally. Native token prompts, element construction, helper sheets and save exercised. |
| F8 | File > Exit (Alt+F4) | done | Standard File > Close, ⌘W | Runs F4 |
| F9 | View > Prop Information | done | P/DesignerCommands.swift, P/DesignerCanvas.swift `LabelsView` | Harness toggle. CATextLayers, reused by index |
| F10 | View > Prop Editor | elsewhere | P/DesignerCommands.swift, tool band | Opens `WindowID.customPropEditor` with a new UUID, as a window you work alongside. No such window in a build without Tools |
| F11 | Edit > Cut / Copy / Paste / Delete | done | P/DesignerCanvas.swift (`cut:`, `copy:`, `paste:`, `delete:`, `selectAll:`), P/DesignerModel.swift `DesignerResponder` | Standard Edit items through the responder chain: the canvas takes them when it has the keyboard, text fields keep their own. Always enabled, as upstream. Run through the responder chain under harness |
| F12 | Edit > Undo / Redo | done | P/DesignerModel.swift `DesignerUndoManager`, P/DesignerCanvas.swift `validateMenuItem` | Edit > Undo and Redo reach the session wherever the keyboard is, as upstream's menu items act whatever has focus, except that a text field being edited keeps its own undo. Titles name the action. Harness: `undo:` and `redo:` sent down the responder chain undid and redid a drawn line. In the real app, Undo was disabled before this change because NSWindow's empty undo manager answered first |
| F13 | Edit > Lock / Unlock / Unlock All / Hide Locked | done | P/DesignerCommands.swift `DesignerEditItems` | ⌘L, ⌘U, ⌥⌘U (⌘N is File > New). Enabled by `toolbar_state`. Shown only while a designer is in front, placed before Find because the sequencer replaces the pasteboard group; `scripts/check_designer_menus.py` |
| F14 | Edit > Background Properties | done | P/DesignerDialogs.swift `ResizeBackgroundSheet`, P/DesignerModel.swift `resizeBackground` | Harness snapshot of the sheet. Validation by code. Resample off the main thread |
| F15 | Edit > Templates (Legacy) | done | P/DesignerDialogs.swift `LegacyTemplatesSheet` | As upstream: the list is empty, so Add to Preview does nothing (Q2, owner's call). Populating it is one line, `PreviewQuirks.populateLegacyTemplates`, with `TemplateStore` already behind it, off the main thread |
| F16 | Settings > Update Location Properties | done | P/DesignerCommands.swift `DesignerSettingsItems` | Harness menu dump |
| F17 | Settings > Location Offset Setup | done | P/DesignerDialogs.swift `LocationOffsetSheet` | Harness snapshot |
| F18 | Settings > Use OpenGL Preview | done | P/DesignerCommands.swift | Always enabled, label kept. Applied in both directions at close (Q8, settled) |
| F19 | Settings > Keep Insert Mode Active | done | P/DesignerCommands.swift | Rust for what it does |
| F20 | Fixture and renderer warning on close | done | P/DesignerModel.swift `confirmClose` | Only the first case exists. Real show has no fixtures, so by code and Rust |
| F21 | Writing Location properties to elements | done | crate `location_updates`; FFI `commit` | Rust. Harness: a line drawn, the window closed, and reopened: the line was there, so the close path wrote the preview back. Locations go into the tree with the data, off the main thread |
| F22 | Help menu | done | P/DesignerCommands.swift | View Help ⌘?. Custom Prop Help does nothing (Q3, owner's call). Under test the address goes to `notes.txt` |
| F23 | Background group | done | P/DesignerWindow.swift `BackgroundGroup`, FFI `choose_background`, `set_background_alpha` | Slider is one uniform. Harness: copy failure shows the alert. Real image swap by code path shared with F2 |
| F24 | Tool selection | done | P/DesignerWindow.swift `ToolGroups` | Every tool drawn on the real window and looked at. Highlight follows `tool_changed`. Native posted ⌘-click opens Single Light setup; actual Cancel still activates Single Light. ⌘⇧-click activates the tool without a sheet, proving exact-modifier behavior (18 steps; inspected both snapshots). The button reads currentEvent modifier flags |
| F25 | Align group | done | P/DesignerWindow.swift `AlignGroup`; FFI `align` | Harness: `Move 3` on the undo stack. Rust for the maths. Layout as the spec's two rows |
| F26 | Bulb Size group | done | P/DesignerWindow.swift `BulbGroup` | Q4 kept: a single-item decrease undoes the wrong way until the owner decides |
| F27 | Keyboard on the canvas | done | P/DesignerCanvas.swift `keyDown`, `performKeyEquivalent`, `performZoomKey` | Delete (both keys), arrows, Esc, ⌘+, ⌘- run under harness. A bounded private-app activation with real key-window ownership and CG-backed events proves ⌘Z, ⌘Y, ⌘Z, ⇧⌘Z: shape counts 1→2→1→2→1; previous foreground app restored. ⌘Y uses the canvas responder override. ⌘V goes through `paste_text`; ⌘C/⌘X/⌘Z use the Edit responder chain; lock shortcuts remain menu items. Offscreen no-key-window probes cannot validate target=nil AppKit menu dispatch |
| F28 | Mouse: left button with the Select tool | done | P/DesignerCanvas.swift `mouseDown`; crate | Harness: select, move, ⌘-click add, band. ⌘-click for multi-select: Shift already means locked prop here (spec F28) |
| F29 | Mouse: left button with a drawing tool | done | P/DesignerCanvas.swift; P/DesignerModel.swift `runElementTemplate`; `Kit/ElementTemplateRunner.swift`; `app/Sources/VixenSetup/Window/TemplateRunner.swift`; crate `element_template_finished` | All shapes draw, click-by-click kinds included. With no element selected, mouse up runs the shape's template wizard as sheets on the designer: Group Setup, the dimming question, the Dimming Curve and Color Handling helpers, then the new group is linked to the shape, shown in the tree and selected. Run under harness on the real show with an arch (shape linked, 25 lights added, saved to `SystemConfig.xml`) and a star cancelled (shape stays, unlinked). With a tree row selected the shape links to it directly and no wizard shows, tool still active. Needs `setup` and `dialogs` in the build: without them the hook is nil and the shape stays unlinked, as on Cancel |
| F30 | Mouse: drag and hover | done | P/DesignerCanvas.swift `setCursor`, `DesignerCursors` | Upstream's two cursors converted from their `.ico`s to `app/Resources/preview`. Four-way arrow drawn. Rubber band and hover by harness |
| F30a | Additional shapes from a template | done | crate `element_template_finished`, `create_additional_shapes`; FFI `DisplaySetupSession.leaves_created_since` | Crate tests plus native generated-profile workflow: draw a head, select Test Head profile, traverse all eight wizard pages, increase count to three, Finish and Save (37 steps). XML checks three distinct fixture-node links, original at (360,80), clones at (0,0)/(80,0), all 80×80; five shapes total. Upstream places additional heads in rows from the origin |
| F31 | Copy | done | P/DesignerModel.swift `copy` | Text on the pasteboard, a named one under test. Q11 kept |
| F32 | Paste | done | P/DesignerModel.swift `paste` | Harness: pasted shape follows the pointer until a click |
| F33 | Cut | done | P/DesignerModel.swift `cut` | Rust |
| F34 | Delete | done | P/DesignerCanvas.swift, `deleteSelection` | Harness: delete key and undo |
| F35 | Lock, Unlock, Unlock All, status rules | done | P/DesignerWindow.swift `LockGroup`; crate `unlock_all_without_undo`, `toolbar_state` | Rust and harness, including the hint once per window. Q6 taken |
| F36 | Hide Locked / Show Locked | done | P/DesignerWindow.swift `LockGroup` | Rust and harness. Icon and help swap |
| F37 | Context menu on the canvas | done | P/DesignerCanvas.swift `rightMouseDown`, `contextMenu` | Built from `context_menu` with separators, no leading or trailing one. No right mouse up sent. Actual posted right-click opens native menus; Group, Ungroup, Create Template and Cancel pass with shape counts 2→1→2→1 (25 steps) |
| F38 | Set Z Location | done | P/DesignerCanvas.swift `setZ` | Q1 undecided: pixels change, location writes not passed on |
| F39 | Group, Ungroup, Create Template (context menu) | done | P/DesignerModel.swift `createTemplate`, `perform` | Harness: group/ungroup plus real Create Template sheet: invalid name rejected, valid name writes XML with both child shapes, Cancel retains the group without writing a template (42+15 steps). Selected group shows no panel until clicked, as upstream |
| F40 | Import builds elements and a shape | done | `preview_import`, `custom_prop_builder` | Private authored `.prp` imported and saved; native tree contains substituted group and leaves, linked preview lights. |
| F41 | Element creation with name tokens | done | `custom_prop_builder`, `DesignerModel.importProp` | Native prompts for two tokens, Rust substitution tests; saved names match responses. |
| F42 | Face mapping from the prop | done | `preview_import::import_properties`, `refresh_face_colors` | Authored AI mapping persisted; post-helper default changed from Blue to RoyalBlue. State Doors/Open persisted with the two newly created node IDs. Rust tests cover duplicate/unresolved targets and defaults. Latest upstream legacy loader intentionally preserves its name-only conversion. |
| F43 | Light nodes | done | `build_custom_prop_mapped` | Imported light-to-node links and remapped state targets inspected in saved private profile; scoped Rust tests pass. |
| F44 | Prompts after creation | done | `DesignerModel.importProp` | Native Dimming Curve prompt, Not Now, Single Color helper and result report exercised; save after report dismissal verified refreshed Face default. |
| F45 | Resize Preview (`ResizePreviewForm`) | done | P/DesignerDialogs.swift `ResizeBackgroundSheet` | Harness snapshot |
| F46 | What a resize does | done | session `resize_background_image`; FFI `resize_background` | Rust for the file. CatmullRom stands for HighQualityBicubic |
| F47 | Templates (Legacy) (`TemplateDialog`) | done | P/DesignerDialogs.swift `LegacyTemplatesSheet` | As F15. Title "TemplateDialog" kept. Load errors would show "Error Loading Template" |
| F47a | Create a Template name dialog (`PreviewCustomCreateForm`) | done | P/DesignerDialogs.swift `CreateTemplateSheet` | Sheet stays open on an invalid name (Q12, settled). Rust for the rule |
| F48 | Location Offset (`LocationOffsetForm`) | done | P/DesignerDialogs.swift `LocationOffsetSheet` | Harness snapshot |
| F49 | Single Bulb Configuration (`PreviewPixelSetupForm`) | done | P/DesignerDialogs.swift `SingleBulbSheet` | Harness: Cancel activates the tool. Emptied prefix drops the size, as upstream |
| F50 | Status legend | done | P/DesignerWindow.swift `DesignerStatusBand` | Looked at. Colours from `Theme.Preview`, same values as `overlay::theme` |
| F51 | Zoom slider and readout | done | P/DesignerModel.swift `zoomSliderMoved`, `applyPendingZoom` | 250 ms debounce and mouse up. Q5 kept: under 10 points does nothing |
| F52 | Mouse-wheel and gesture zoom | done | P/DesignerCanvas.swift `scrollWheel`, `magnify`; session `magnify` | Harness wheel. Rust for pinch carry. Pinch itself needs a trackpad, not run |
| F53 | Scroll bars | done | P/DesignerCanvas.swift | Overlay scrollers over an empty document. The odd equal-size rule goes, as the spec says |
| F54 | Blank background grows with the shapes | done | crate `view_resized` | Rust. Runs at the end of a live resize only |
| F55 | All-black background cleanup | done | session `load_background_checked` | Rust |
| F56 | Background load failure | done | FFI `load_background` | Rust. Q13 kept: the name is forgotten |
| F57 | Drop .prp files on the canvas | partial | `DesignerCanvas` drop destination, `DesignerModel.importProp` | Registered file drop forwards to the same importer; file picker/native import path verified. Physical Finder drag pending. |
| F58 | Custom Prop Library button | done | P/DesignerModel.swift `openCustomPropLibrary` | Finder on Documents |
| F59 | Custom Prop group, as a whole | done | P/DesignerWindow.swift `CustomPropGroup` | All three controls implemented; native import verified with authored package. |
| F60 | Lifetime of a live window | done | P/LivePreviewCoordinator.swift, P/LivePreviewWindow.swift | Follows `running_previews` and `subscribe_previews`. AppKit windows rather than a SwiftUI group, because they need `canBecomeKey` when borderless and no `.closable`. So `WindowID.livePreview` has no scene any more, and nothing opens it. ⌘W gets the alert through `performClose` |
| F61 | Title | done | P/LivePreviewWindow.swift `statusTick` | Follows a rename within a second |
| F62 | Status bar ("Show Status") | done | P/LivePreviewWindow.swift `LiveStatusBar`; crate `linked_pixel_count`; FFI `status` | Looked at: `Lights: 2307`, `Distance 718.2`, `FPS: 0` on the 2022 tree (camera), `Lights: 977  FPS: 3 fps` on the 2019 tree (flat). Read at 1 Hz. The camera view's bar lies over the canvas, the flat view's canvas stops above it |
| F63 | Right-click context menu | done | P/LivePreviewWindow.swift `showMenu` | All items, by view. Prior action checks plus actual posted right-click opens the flat-preview menu and invokes Show Borders twice (16-step native workflow) |
| F64 | Show Borders | done | P/LivePreviewWindow.swift `applyBorders` | Native context menu toggle and restore on generated flat preview. Inspected bordered/borderless snapshots; frame changes 896×482→896×450→896×482 while content stays 896×450. Existing real-show check verifies saved ShowBorders |
| F65 | Drag the window by its content | partial | P/LiveCanvasView `mouseDragged` | `performDrag` past 2 px. Not run: needs a real mouse |
| F66 | Flat view zoom | done | session `LiveView`, `flat_zoom_step` | Rust |
| F67 | Camera view | done | session `camera_*`, `point_scale`; render `set_light_scale` | Rust, and looked at on the 2022 tree: picture at 1:1 at the default distance |
| F68 | Always On Top | done | P/LivePreviewWindow.swift `toggleAlwaysOnTop` | Harness |
| F69 | Auto On Top (`IsOnTopWhenPlaying`) | done | P/LivePreviewWindow.swift `playerActive`; session `sequencer/state.rs`, `preview_devices.rs` | Source integration verified: sequencer start/stop calls `notify_player_active`, window subscription updates level. Native level action remains covered by the live-window harness |
| F70 | Transparent Background | partial | P/LivePreviewWindow.swift `setTransparent`, `showBadge`; render `set_transparent`; Window menu | Rust for the render. Click-through not yet checked on a real window |
| F71 | Window position and size persist per preview | done | session `load_live_window_state`, `save_live_window_state`, `restore_bounds`; `saveSoon` | Rust. Looked at: camera window opens at the background size. One write, debounced 300 ms |
| F72 | Live window keys forwarded to the sequencer | done | P/LiveCanvasView `keyDown`; `VixenSequencer/SequencerScene.swift` | Source integration verified: notification carries the NSEvent and the sequencer observer dispatches to its active window |
| F73 | Frame updates | done | session `LiveTarget::compute` | Run on the real show. A frame whose colours for this window don't change isn't sent to the renderer at all, so the renderer's dropped count means frames really replaced before they could show. Measured in docs/benchmarks/preview-ui.md: every change drawn |
| F74 | Live pixel colour rules | done | session `light_color` | Existing, and looked at with a real sequence |
| F75 | Instrumentation values | done | FFI `open`, `status`; session `LiveTarget::with_update_time` | Frame update and render CPU values published and removed with the last window. Pixel set time remains folded into frame update; GPU background/points sub-timers intentionally omitted because the pass is one submission (at most four draws including fixtures). Native fixture status showed ongoing frames and no renderer errors |
| F80 | Panel placement | done | P/DesignerWindow.swift `DesignerSplitController`, `DesignerFloatingPanel`; P/DesignerPanels.swift `FloatButton`; P/DesignerCommands.swift | Left column, 50% split, View menu toggles. Each panel's corner button, or View > Float Elements Panel and Float Properties Panel, takes it out into a floating panel over where it was, and the panel's close button docks it again. Harness: both floated (the canvas takes the whole window), the floating Elements panel looked at with its tree, selection and context menu working, the Properties panel closed and back in the column, both docked again with the tree as it was. Dragging a panel out by its title, as DockPanel allowed, is not offered |
| F81 | Add template row | done | P/DesignerPanels.swift `ElementsPanel`; P/DesignerModel.swift `addElementTemplate` | Harness (`designerCommand` `addTemplate Arch`): Group Setup, Not Now, Color Configuration, the Color Setup summary, then "Arch" is in the tree and selected. Each template remembers its last values for as long as the window is open (`ElementTemplateMemory`), where the canvas path starts fresh each time, as upstream. Off while a wizard is running or when Display Setup isn't in the build |
| F82 | Element tree | partial | P/DesignerPanels.swift `ElementsPanel`, `ElementsProvider`; P/DesignerModel.swift `treeEditor`, `elementsEdited`; `Kit/ElementTreeEditor.swift`; `app/Sources/VixenSetup/Window/TreeEditRunner.swift`; FFI `DisplaySetupSession.apply_live`, `rows_for_node_paths`, `node_paths_of` | VixenKit tree with rows, lazily. The context menu, the keys (⌘C, ⌘V, Delete, and the rest of `OutlineCommand`) and drags within the tree are Display Setup's own code, working on a copy that becomes the running display after each edit, with nothing saved until the designer saves. Menu in upstream's order with Cut off, no property items (upstream `AllowPropertyEdit` false here) and Export Wire Diagram on. Harness on the real show: the menu's contents and enabled states with nothing, a group and a leaf selected; Rename (saved as typed); Add (new row selected and scrolled to); Delete with its alert; Paste into a leaf raising "Convert Element to Group?"; Export Wire Diagram's no-location alert; Export Element Tree writing its file; Save keeping all of it. A second run did a drop onto a leaf (through `canDrop` and `drop`, the calls a drag ends in: Star went under Plug 11 after the convert alert, its 40 lights still linked), Sort, Create Group (the new group goes at the top level with the element added to it, as upstream), Paste as New, and Add Multiple through its sheet, then Save: the saved tree has all of it. Timings in `docs/benchmarks/preview-ui.md`. Native generated-profile verification also covers Find/Replace, Reverse Elements, and tag Save: persisted sibling order Node-3/Node-2/Node-1, all three Prop tags, custom #123456 tag color. Profile hashes remain unchanged before designer Save. Native posted right-click opens Find/Replace and Cancel from the tree (14 steps); physical tree dragging remains unverified. Tag dots use the shared seeded tag table; dedicated Swift build passes. Read-only when Display Setup isn't in the build |
| F83 | Tree selection highlights lights | done | P/DesignerModel.swift `elementsSelected` | Row last acted on goes first |
| F84 | Selecting a shape clears the tree selection | done | crate; model `ignoringTreeSelection` | Harness |
| F85 | Deleting elements in the tree unlinks them | done | crate `tree_replaced`; FFI `reload_elements`; P/DesignerModel.swift `elementsEdited` | Harness on the real 2019 preview: deleting Relays/Star took the linked lights from 977 to 937, and the saved preview holds 937 links, none to a missing element. Only the open preview is unlinked, as upstream |
| F86 | Manage tags from the tree | done | `TreeEditRunner.retag`, `manageTags` | Harness: Tags ▸ Prop and Manage Tags Cancel, plus native Save from the designer. The existing test hook sets a color, actual Save commits it to staged display, designer Save persists #123456 and all three node tags. Physical system color-panel selection remains outside this test |
| F90 | Panel title and swap | done | P/DesignerPanels.swift `DesignerPropertiesModel` | Looked at: "Line Properties" |
| F91 | Property grid | done | P/DesignerPanels.swift `PropertyGridRows`, `PropertyRow`; crate `properties_for_grid` | Rows in the grid's order, categorized and alphabetical, description pane, commit on Return or focus loss, paired posted native mouse events double-click True/False and list rows, proving Locked false→true and StringType Standard→Pixel (21+23 steps). A dense grid rather than a grouped `Form`, which doesn't fit a 240 pt column. Point rows show "x, y" and X and Y fields |
| F92 | Group properties (`PreviewCustomSetupControl`) | done | P/DesignerPanels.swift `GroupStringHeader` | Native string picker selects Test String 2; after its size is set to 9, native Sync sets both children to 9. Picker returns to Test String 1 and read-only state plus saved XML confirm it (21 steps) |
| F93 | Linked Elements dialog (`PreviewSetElements`) | partial | P/DesignerDialogs.swift `LinkElementsSheet`, `LinkedRowsTable`; FFI `PreviewLinker` | Native Copy-to-String and Reverse menus, OK persistence, reopen and changed-clone Cancel pass (49 steps). Fixed context-menu row capture to use the right-click position rather than stale clickedRow. AppKit row selection plus the shared double-click callback assigns leaf/group nodes; physical tree double-click and drag remain unverified in the offscreen harness |
| F94 | Fixture Link Element dialog (`SelectFixtureNodeView`, WPF) | done | P/DesignerDialogs.swift `FixtureLinkSheet` | Generated profile: actual List selection and OK replace one head link with Fixture Two, selecting Fixture One then Cancel preserves it; native ⌘-deselect and OK clears the link. Read-only properties and saved XML confirm each result (25+19 steps) |
| F100 | Undo and Redo split buttons | done | P/DesignerWindow.swift `UndoGroup`; crate `undo_many`, `redo_many` | Rust. The running label is lost in a plain Menu, so each row says what it does |
| F101 | Undo stack rules | done | crate; `DesignerMenuBar` | Rust is the only stack |
| F102 | What each undo action is | done | crate | Rust. Labels seen under harness |
| F103 | Rotate handle | done | crate; canvas | Rust. ⌘ alone snaps |
| F104 | Modifier behaviour during a handle drag | done | crate; canvas `flagsChanged` | Rust |
| F105 | What a new shape starts as | done | crate | Rust, and each kind looked at |
| F106 | Property edits with side effects | done | crate `set_property`; model `alertTitle` | Rust |
| F107 | Prop Information overlay | done | P/DesignerCanvas.swift `LabelsView` | Harness toggle. Not looked at with the box text yet |
| F108 | Edit colours and legend meaning | done | crate `overlay`; session `DesignerHost` | Rust against the GPU: white, lime, yellow and larger first pixel |
| F109 | Draw and hit order | done | crate | Rust |
| F110 | Lock/Unlock group, as a whole | done | P/DesignerWindow.swift `LockGroup` | Looked at |
| F111 | Mac-only additions, none in upstream | done | P/DesignerCommands.swift, P/DesignerCanvas.swift, P/DesignerWindow.swift | Select All, View toggles, Window menu command, gestures, floating panels (F80). Pinch itself needs a trackpad, not run (F52) |
| F120 | Where fixtures draw | done | `render/fixture.rs`, `renderer.rs`, `session/preview_design.rs`, `preview_fixtures.rs` | Camera: perspective depth-tested volumes. Flat live view: omitted as upstream. Designer: local perspective thumbnails clipped to each rectangle, F108 beam colors and existing HotPink selection outline. Required-GPU tests cover flat omission, two beam colors, clipping/zoom/pan; native live and designer images inspected in `target/preview-nov11/` |
| F121 | Per-frame fixture state | done | `crate/fixture_state.rs`, `session/preview_fixtures.rs` | LiveTarget feeds per-element states to a dedicated fixture worker. Preparation reads wheel entries plus immediate shutter/dimmer filters; pure and required-GPU integration tests pass. Native generated fixture sequence rendered without errors |
| F122 | Pan and tilt movement | done | `fixture_state.rs`, `preview_fixtures.rs` | Worker advances travel at the configured update interval even after input ends, then parks. Native stop/return-to-home captured; profile pan/tilt/zoom constraints copied when the shape is linked |
| F123 | Strobe | done | `fixture_state.rs`, `preview_fixtures.rs` | Shared duration/rate timers with on/off deadlines, no sleeping GPU/UI thread. Required-GPU test proves one engine frame drives later flashes and clearing parks. Native sequence strobe captured |
| F124 | Legend | done | `fixture_state.rs`, FFI `fixture_legends`, P/LivePreviewWindow.swift `FixtureLegendView` | Show Legend controls red Courier labels, projected with the mesh camera. Text sorting retained. Native screenshot shows pan/tilt labels; strobe label appears during the strobe effect. Legend polling only exists for fixture windows |
| F125 | Fixture geometry parameters | done | `render/renderer.rs`, `fixture.rs`, `session/preview_live.rs` | Reference background/form dimensions, rounded maximum beam, transparency and body dimming wired. Required-GPU test verifies body dims while beam color stays unchanged. Background alpha also applies when there is no image |

## Other-area integration

The prop loader and element setup helpers are wired. Sequencer playback notification and forwarded preview keys are wired. Fixture editors reuse the renderer API and are owned by the specialized editors pass.

## Notes for the next pass

- Live windows are AppKit windows owned by `LivePreviewCoordinator`, following `running_previews` and `subscribe_previews`. `WindowID.livePreview` has no scene any more, and nothing in the app opened it.
- The old test hooks (`VIXEN_TEST_PLAY`, `VIXEN_TEST_PLAY_FROM`, `VIXEN_TEST_PREVIEW`, the forced float) are gone. Their jobs are harness steps now: `playSequence`, `liveOnScreen`, `measureLive`.
- Tests have added lines to the 2019 preview in `target/show-preview` and saved them. Copy `target/show-copy` again for a clean show.
- `target/show-preview-big` is a copy of the show with the synthetic 100,000-light preview added.
- Speed: `docs/benchmarks/preview-ui.md`.
- Two VixenKit hooks carry Display Setup's code into the designer, both nil when `setup` isn't in the build: `ElementTemplateRunner.run` (one template's wizard as sheets on a window) and `ElementTreeEditor.make` (the tree's menu, keys and drags). VixenSetup registers both in `SetupScene.init`. Each works on a `DisplaySetupSession` copy and ends with `applyLive()`, which makes the copy the running display without saving.
- `TreeEditRunner` keeps one working copy for as long as `core.displayRevision()` stays where it was. The designer's own save and commit write to the system without moving that revision, so the designer calls `treeEditor.invalidate()` after a save. Anything else that writes to the system outside a display apply would need to do the same.
- `applyLive` replaces the whole running system with the copy and stops playback, as any display apply does. The tree's clipboard is in the copy, so it is lost when the copy is reopened (after a template wizard or a designer save).
- A tree command issued while another is still running is dropped. In a harness script put a short wait between two `treeKey` steps.
- A sheet put on a window that isn't yours (`TemplateRunner.sheet`) must have `sizingOptions = []` and be sized once from `fittingSize`: one that follows its content's size threw an exception inside AppKit as it animated in. The Dimming Curve helper's long text is cut short in such a sheet (cosmetic, open).
- `designerPixels` reads nothing back unless the window is really on screen (`designerOnScreen`).
- Building a narrower set of areas after a wider one under the same `VIXEN_BUILD_ID` fails at link: `VixenApp.swift` asks `canImport(VixenSetup)`, and the old `VixenSetup.swiftmodule` is still in `target/build-<id>/swift/out/Products/Debug`. Delete the left-over module's `.swiftmodule`, `.o`, `-Swift.h` and `.modulemap` there, or use another build id. Checked this pass: `preview,admin` alone builds and runs with the tree read-only (`treeMenuDump` answers `no menu`), then the four-area build again.
- Computer use was refused in pass 2 (the access request came back denied), so nothing in that pass was checked with a real mouse or a real right-click: only through the harness. The rows that say so are still open on that point.

- Fixtures: `MovingHeadParams` is the shared renderer input. `thumbnail` selects a local clipped camera for editors; `BeamWidthModel` retains preview/editor focus formulas. `tilt_offset_deg` stays explicit; all port callers use the owner-confirmed +180° for bottom mounting. The source's overlapping final cylinder panel and displaced cap centre remain; duplicate beam-half rendering is removed as the specialized spec requested.


## Quirks and decisions

Kept as upstream unless the spec calls it settled. Each owner's call stays open.

| Q | Choice here |
|---|---|
| Q1 | Kept open: Set Z changes pixels, and no location writes follow |
| Q2 | Upstream: the dialog is empty. One line to change, `PreviewQuirks.populateLegacyTemplates` |
| Q3 | Upstream: Custom Prop Help does nothing |
| Q4 | Upstream: a single-item bulb decrease undoes the wrong way |
| Q5 | Upstream: slider moves under 10 points do nothing |
| Q6 | The spec's: menu and key both take the no-undo path, since they are one command here |
| Q7, Q8, Q9, Q12, Q14, Q15 | Settled in the spec, done that way |
| Q10 | Upstream: property edits aren't undoable |
| Q11 | Upstream: a copied item stays in the multi-list |
| Q13 | Upstream: a missing or unreadable background forgets its file name |
| Q16 | Upstream: `resolve_group_links` on open |

## Upstream forms

From `docs/reference/forms/_index.json`. Controls are covered by the F rows above.

| Form | Where |
|---|---|
| VixenPreviewSetup3 | `P/DesignerWindow.swift`, `P/DesignerCommands.swift` |
| VixenPreviewControl | `P/DesignerCanvas.swift` |
| VixenPreviewSetupDocument | The canvas. Its one-tab strip is omitted: DockPanel chrome (F80) |
| VixenPreviewSetupElementsDocument | `P/DesignerPanels.swift` `ElementsPanel` |
| VixenPreviewSetupPropertiesDocument, PreviewShapeBaseSetupControl | `P/DesignerPanels.swift` `PropertiesPanel` |
| PreviewCustomSetupControl | `P/DesignerPanels.swift` `GroupStringHeader` |
| PreviewSetElements | `P/DesignerDialogs.swift` `LinkElementsSheet` |
| ResizePreviewForm | `P/DesignerDialogs.swift` `ResizeBackgroundSheet` |
| LocationOffsetForm | `P/DesignerDialogs.swift` `LocationOffsetSheet` |
| PreviewPixelSetupForm | `P/DesignerDialogs.swift` `SingleBulbSheet` |
| PreviewCustomCreateForm | `P/DesignerDialogs.swift` `CreateTemplateSheet` |
| TemplateDialog | `P/DesignerDialogs.swift` `LegacyTemplatesSheet` |
| GDIPreviewForm, OpenGlPreviewForm, GDIControl | `P/LivePreviewWindow.swift` |
| PreviewCustomDefineDisplayItems | omitted: empty form, never opened upstream |
| ConfigPreviews | Administration's area |
| PreviewWaveform | Not this area: the Beats and Bars analysis module |

## Windows-only omissions

As the spec's table, each with its reason there: colour key transparency, `CP_NOCLOSE_BUTTON`, the OpenGL support probe, the 32-bit zoom limit, DPI tools, theme renderers, DockPanel Suite, `IsPreviewThreaded`, the GDI and OpenGL paths, `MessageBoxForm`'s capitals. Each was replaced as that table says.

## How to run

Harness steps this area adds, on top of `docs/UI_GUIDE.md`: `previewTarget` (which preview the next `window` step with `preview-setup` opens), `designerTool`, `designerCommand`, `designerInfo`, `designerBackground`, `designerZoom`, `designerOnScreen`, `liveOnScreen`, `liveWindowInfo`, `liveMenu`, `liveSize`, `playSequence`, `stopSequence`, `measureLive`, `measureDrag`, `measureOpen`. See `P/PreviewHarness.swift`.

`designerCommand` also takes, for the Elements tree: `addTemplate <name>`, `treeSelect <Name/Child name>` (by names from a root, expanding on the way), `treeMenu <Title[/Sub title]>` (performs that context-menu item for the tree's selection), `treeMenuDump` (the menu with `(off)`, `[on]`, `[mixed]` to `notes.txt`), `treeKey <copy|paste|pasteAsNew|delete>`, `treeDrop <Name/Child> > <Name/Child|root>` (asks `canDrop` then `drop`, as the end of a drag; `label: "copy"` for a copy; answers `refused` when the tree wouldn't take it), and `floatElements`, `floatProperties`, `dockElements`, `dockProperties`. A `focus` step with `window: "Elements"` or `"Properties"` aims the next steps at a floating panel. Under the harness each tree edit writes its time to `notes.txt`. `designerInfo` reports `linked=<lights linked to an element>`. The tree commands need `VIXEN_AREAS=preview,admin,setup,dialogs`.


## Verification, November 11

- Dedicated `preview,admin,setup,dialogs` app builds. Private generated profile: `vixen-session/examples/fixture_preview.rs` (refuses existing folders and writes under target only); no controllers, hardware or media.
- Required-GPU renderer suite: 50 headless, 16 live and 1 idle tests pass on Metal. New fixture tests cover perspective, color halves, darkness, body dimming, editor clipping and canvas zoom/pan.
- Session fixture setup and required-GPU worker tests pass; the worker test feeds one frame, observes later flashes, clears it and verifies rendering parks. 36 scoped preview session tests and four prop import tests passed earlier in this pass.
- Native fixture playback harness: 16 steps, zero failures. `target/preview-nov11/fixtures` contains live pixel readbacks plus separate native legend screenshots (AppKit snapshots do not include Metal content). Both mounted orientations, movement, strobe and stop were inspected.
- Designer thumbnails and import token/color sheets inspected under the native harness in `target/preview-nov11/import`. Authored face/state remapping and discrete-color refresh have direct Rust regression tests.
- Authored native import: 23 initial steps passed; final 27-step run passed all 25 import/helper/save actions, with two unrelated initial on-screen/readback failures before import. Saved XML proves token substitution, AI face mapping, helper color refresh to RoyalBlue and Doors/Open targets remapped to the two new nodes. Images inspected in `target/preview-nov11/import-final`.

## Native dialog verification — November 11, 2026

`examples/preview_dialogs.rs` creates a new controller-free profile only under `target`, with two linked strings, two fixture elements, and an optional invalid group link. `target/preview-partials` contains scripts, private profile copies, logs, and inspected snapshots. The fixture wizard uses the existing local Test Head fixture definition; no show files, audio devices, controllers, or external services are used. Dedicated preview/admin/setup build passes (`build-linker.log`).

`python3 scripts/check_preview_dialogs.py` independently checks native action reports and persisted XML for close/cancel, properties, tree order/tags, template XML, group Sync, additional fixture positions and unique links, fixture link OK/Cancel/clear, and linker copy/reverse/OK/Cancel. The test-only `designerProperties`/`designerLinkerInfo` actions are read-only; `previewLinker doubleClick` invokes the same sheet callback with real selected table/tree rows. Physical linker drag/double-click remains partial. The context-menu row calculation was discarded in favor of potentially stale `clickedRow`; `ContextTable` now passes that row explicitly.

Window-position restoration, content dragging, transparent click-through, Finder drops, and physical tree gestures remain explicitly unverified. No computer-use retry was made after the earlier service timeout. Native control tests do not claim those gestures.

The bounded foreground keyboard probe `keys-active2-out` established a real key window and canvas responder: native CG ⌘Z/⌘Y/⌘Z/⇧⌘Z produced shape counts 1→2→1→2→1. Its helper confirmed the exact test process exited and restored the previously frontmost application. Save was not accepted as verified by that run because main-window ownership was not recorded. `keys-settled-out` later attempted makeMain plus a 200 ms Commands settle, but reported key/main nil throughout; its previously frontmost app was SecurityAgent. That run is discarded as a focus-limited probe and is not included in the 421-step checker. No production Undo change was made. Hand-built NSEvent character strings also caused a false dedicated Save failure; the unchanged CGEvent→NSEvent path succeeds.
