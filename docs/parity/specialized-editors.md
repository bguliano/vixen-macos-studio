# Parity: specialized editors

Spec: `docs/spec/specialized-editors.md`. Status values: **done** (with where it lives), **partial** (with what is
missing), **todo**, **omitted** (with why), **other area** (built elsewhere; checked here).

## Original handover notes

- Pass 1 started this table with every F number as todo. Rows are updated as groups of features land.
- **No real `.prp` files exist** anywhere on this machine: none under `target/show-copy`, none in the owner's show,
  none in `upstream/`. Both the package format and the legacy LiteDB v4 reader are therefore tested only against
  files built by the tests from upstream's reader/writer rules, never against a file written by real Vixen.
- **Crate placement.** The spec proposes `vixen-core::custom_prop`, `vixen-io::custom_prop_file` and
  `vixen-session::custom_prop`. The model, the file formats and the window-free editor core live instead in a new
  crate, `crates/vixen-prop`, so that `vixen-core` and `vixen-io` (shared by every worker) are not edited and the
  editor's tests build in seconds. Profile preferences and polygon editing now live in `vixen-session::tools`; there is no `custom_prop` re-export module.
- **Built so far in `crates/vixen-prop`** (`cargo test -p vixen-prop`): `model.rs` (the element graph with several
  parents per element, orphan removal, name counting and `Uniquify`, order numbers, light sizes, the add-light
  rules), `file.rs` (schema-1 `.prp` package read and write, atomic save, the legacy backup on first save of a
  converted file) and `legacy.rs` (read-only LiteDB v4 reader). These back F4, F5, F6 and the F100-series file
  rules; the rows below say **done** only once the window that uses them is also built and looked at.


## Verified continuation checkpoint — 2026-11-11

`VixenTools` now contains an operating Custom Prop Editor: per-window document, native canvas/tree/order/state panes, package file I/O, clipboard actions, metadata, undo, and awaited dirty close/shutdown guards. Remaining partial rows below are intentionally not promoted merely because they compile.

- Isolated `VIXEN_BUILD_ID=tools VIXEN_AREAS=tools,setup,dialogs,effecteditor,admin scripts/build_app.sh debug` passes.
- Current prop suite: **103 tests pass**, including 5 state, 3 scaling and prop wire-point gathering regressions. Two session preference persistence tests and FFI state color parsing/invalid-edit regression pass. Shared Setup naming15 tests and wire48 tests pass (one existing wire performance test ignored).
- Native harness: `target/resume-ui/props/script.json`, results `target/resume-ui/props/out/result.json`: **48 steps, zero failures**. Synthetic drawing, undo/redo, authored state assignment, save/reload, shutdown Cancel/Save and window Cancel/Discard were verified. `layout.png` and `states.png` were inspected.
- Additional native suites: `target/resume-ui/props-controls` (25 steps), `props-advanced` (49 steps), and `shared-naming` (16 steps), all zero failures. Explicit post-run assertions in `props-advanced/out/assertions.txt` pin preferences, scaled coordinates, native UndoManager sizes, rename/cancel and exported SVG labels. Screenshots were inspected. The offscreen harness has no key window; prop menus use a test-only registry fallback and Test Undo/Redo invokes the same document UndoManager as the toolbar.
- Test profile is `target/prop-test-profile`; saved prop is `target/resume-ui/props/Test Prop.prp`. Original show and hardware were untouched. No real Windows-authored `.prp` is available, so real-file compatibility remains unverified.

- xModel continuation: `target/xmodel-tests.log` records all94 prop tests passing (12 xModel tests). `target/resume-ui/xmodel` contains 79 native steps plus strict `check.py` assertions and 21 fresh-process steps. Picker, imported State tab and custom/circle/Stardust canvases were inspected. The reference named GE Stardust 1500 contains 1499 distinct orders, independently counted from its XML; imported count matches. No vendor requests were made.

- Vendor continuation: `target/vendor-tests.log` records **102 prop tests passing** (82 foundation + 13 xModel + 7 vendor). `target/resume-ui/vendor` contains 74 core and 83 advanced native steps, zero failures; `check.py` and `check_advanced.py` independently assert saved metadata, unchanged authored graph/states, selection/cancel/error behavior and close/shutdown cancellation. Screenshots inspected. Only a loopback fixture server was contacted; external redirects are rejected in test mode. Production vendor availability has not been probed.
- Native testing exposed an intermittent missing owner from the generic WindowFinder. The prop window now attaches its model/delegate/close guard from `PropWindowAttachment.viewDidMoveToWindow`; the advanced suite verifies the attached host and repeated close/quit cancellation.
- Specialized panel continuation: `target/prop-panels-build.log` passes; `target/polygon-names-tests.log` records 14 Polygon tests. `target/resume-ui/prop-panels` contains 103 native metadata/state-definition steps and 62 Polygon draft-name steps, all passing; `check.py` independently checks saved metadata, Return/Tab Notes, background dimensions/opacity, authored assignments and draft completion/discard. Screenshots inspected. The State selector uses a second action row and a 300-point minimum sidebar to keep names and all grid columns readable.
- State continuation: `prop-panels/state-actions.json` (54 steps) and `color.json` (33 steps), strict `check_state.py`, zero failures. Color well opens the actual offscreen system RGB panel; two edited hex values update the same item and its live preview, then persist. Native header callbacks retain ascending/descending direction. All screenshots inspected.
- Clipboard continuation: `target/resume-ui/prop-clipboard/script.json`74 steps, zero failures; `check.py` verifies keyboard/context deletion distinction, subtree identity restoration, a separate-window paste and fresh identities/orders for Paste as New. Screenshots inspected. Scoped model-role regression brings the prop suite to103 tests (`target/prop-model-tests.log`); it pins leaf rejection, prior-Model demotion, authored-state preservation and undo.
- Edit > Paste Prop Elements as New… (⇧⌘V) stays after `.pasteboard`: the sequencer replaces that group, but this item still reaches the menu bar, only while a prop editor is in front. `scripts/check_prop_editor_menus.py` reads the harness's menu dumps with the editor open and closed.

- Element/group continuation: `target/resume-ui/prop-info`165 native steps, zero failures; `check.py` independently asserts F2 timing/cancel/commit/conflict and undo, metadata/model rules, prompt cancel, alias identity and moved hierarchy. Screenshots inspected. Shared Outline rename behavior is opt-in; Setup naming15 Rust tests and16-step native sheet smoke pass after the change.

- Canvas/Order continuation: `target/resume-ui/prop-canvas` 89 posted pointer steps, 58 native Order edit steps, 66 geometry-menu steps, 27 Order pasteboard/delegate steps and 31 tree-provider steps pass strict `check.py` saved-geometry/graph assertions. Physical system drag sessions were not verified by that suite; the later nine-case foreground run below adds bounded evidence. The direct 50,000-light CPU raster benchmark and its limits are recorded in that folder. The 22-step posted-button size suite and final 9-step 50,000-light measurement also pass. An in-process AX getter returned NSAttributedString through a declared NSString accessor and threw during Swift bridging. The shared harness now reads raw Objective-C text values; original dumps and positive naming lookups pass. Offscreen AX omits the size icon buttons, whose behavior is verified with posted native pointer events.

- Physical tree/Order continuation: `target/prop-drag-foreground` passes nine actual WindowServer drags and 175 harness steps on a unique private app/profile; no selection/drop hooks are used. Four invalid drops stay clean, three tree moves preserve identities and produce the exact selected destination graph, and two Order drags produce exact contiguous orders. Independent `target/prop-drag-independent-review.json` passes83 checks, including all saved JSON fields except modified date, unchanged background bytes, original files and owner preferences. See `docs/review/prop-native-drag.md` for exact cases; the next continuation adds insertion/reverse coverage, with cross-window limits retained. No production change was needed.

- Physical insertion continuation: `target/prop-drag-edges` passes six more system drags /118 steps: same-parent and cross-parent before/after insertion, Command-reversed two-row center drop, and a kind-compatible Branch→Inner cycle rejection. All saved graph/identity/order/dirty/selection checks pass; independent review passes74 checks with unchanged source files/background bytes/owner preferences. Together the runs verify15 drags /293 steps and close the F34/F35 insertion/reverse gap. Broader cross-window/alias/mixed-selection/autoscroll cases remain outside the physical claim; no production fix was needed.

## A. Custom Prop Editor: window, menus, status bar

| F | Feature | Status | Where / what is missing |
|---|---|---|---|
| F1 | Open the Custom Prop Editor | done | `VixenTools/ToolsScene.swift`, Administration Tools item; independent UUID windows. Native window verified. |
| F2 | Window layout | done | Native 1:2 horizontal split, resizable left element/info panes, toolbar above canvas, status below. Inspected `props-controls/out/layout.png`. |
| F3 | File > New (Ctrl+N) | done | Native prompt preselects default, appends token, and awaits dirty confirmation. 17-step prompt harness verifies replacement text and clean new document. Blank-input fix documented (F145). |
| F4 | File > Open | done | `PropEditorModel.openDocument/open`, background Rust package/legacy loading, dirty confirmation, preserves document on load error. Synthetic package reopened in native harness. |
| F5 | File > Save (Ctrl+S) | done | Rust atomic save, state validation, async native Save; harness saved then reopened authored state, and saved during shutdown guard. |
| F6 | File > Save As | done | Native Save As panel, cleansed filename, profile default directory, same validation and atomic save path. |
| F7 | File > Import xModel | done | Native File item, async file/geometry work, dirty replacement guard, busy overlay, imported unsaved Select-mode document. 79-step native import suite verifies cancel/errors/success/save; 21-step fresh-process suite verifies reload and supplied Stardust model. |
| F8 | File > Exit (Alt+F4) | done | Native window close (⌘W); document delegate waits for save outcome. |
| F9 | Close prompt | done | Save/Don’t Save/Cancel sheet, awaited save; native close Cancel retained dirty window, Discard closed it. Generic AppModel shutdown guard vetoes quit on Cancel. |
| F147 | Dirty tracking and its gaps (audit addition) | done | Rust document revision and saved revision cover all mutations, with snapshot undo. State edits share revision. Native edited indicator follows it. |
| F10 | Edit menu: Cut, Copy, Paste | done | Native window responder chain and private JSON pasteboard; tree validators match Rust capabilities. The74-step clipboard suite dispatches through the actual focused-window responder because the inactive harness has no global key window. |
| F11 | View menu | done | All three original actions (Assign Background, Scale Background Image, Background Opacity) live in the contextual native Prop menu; background/opacity native path and prior scaling suite verified. |
| F12 | No undo in the original | done | Rust snapshot undo/redo and native UndoManager; harness 3 lights → undo 2 → redo 3. |
| F13 | Tools menu | done | Contextual native Prop menu contains Vendor Browser, Front/Back Export Wire Diagram and Preferences; native actions/sheets verified. Global Administration Tools remains available. |
| F14 | Assign Background (menu and toolbar) | done | Native image picker starts in Pictures; async assignment preserves lights, updates canvas/image, records undo, and persists normalized package image. Synthetic checkerboard assignment and opacity were inspected in the 103-step panel suite. |
| F15 | Scale Background Image… | done | Rust options calculator and native sheet; source-based percent/aspect ratio/bounds tests, native 800×600→400×300 scales centres once, undo/redo verified. |
| F16 | Background Opacity submenu | done | 10% through100% checkmarked native menu, no checkmark for off-step values; persists in prop. |
| F17 | Status bar | done | Pointer coordinates at left, vendor status at right; native layout inspected. |
| F18 | Prop Info panel, tab General | done | Native General form including read-only dates; fields stored by Rust and document revision. Inspected native form. |
| F18a | Prop Info panel, tabs Physical, Vendor, Notes | done | Native 103-step suite edits every Physical/Vendor field and multiline Notes, then checks saved values. Notes accept literal Return/Tab with native spelling; zero-sized text-view initialization fixed. |
| F18b | Element Info panel | done | Native54-step suite verifies name conflict, face/color normalization, Model demotion, leaf rejection, SubModel acceptance, read-only counts and multiple-selection clearing. Saved roles/colors asserted; Rust regression pins authored-state preservation and undo. Resolver enforcement difference recorded. |

## B. Custom Prop Editor: element panel

| F | Feature | Status | Where / what is missing |
|---|---|---|---|
| F22 | Layout tab: tree | done | Virtualized handle-driven NSOutlineView; Rust bulk tree snapshot, ancestor expansion and selected group shown in inspected native layout. |
| F23 | Inline rename | done | Opt-in native F2 gate waits750ms after selection; Escape restores without mutation, Enter commits, conflicts uniquify, undo/redo restore names. Native55-step suite verifies all cases; default Outline consumers unchanged. |
| F24 | Layout tab keyboard | done | Delete key removes selected lights while retaining their groups; context Delete removes groups. Native key/Undo tested. X/C/V use the tested native responder chain. Upstream defines no separate expansion shortcuts. |
| F25 | Context menu "Create Empty Group" | done | Native prompt preselects suggestion; Cancel leaves clean graph, Create adds empty child and retains selected expanded parent. Saved graph asserted in56-step group suite. |
| F26 | Context menu "Copy to New Group" and "Move to New Group" | done | Native blank prompts match source. Copy creates alias references and clears selection; Move nests all siblings under their parent and expands/selects new group. Native56-step suite and saved identity/structure assertions pass. |
| F27 | Context menu Cut | done | Native menu and responder share full-subtree capture before removal.74-step suite cuts a group, pastes it, and proves node/light identities and content are restored. |
| F28 | Context menu Copy | done | Native menu/responder write private JSON without mutation; a copied group pastes in a second editor with original identities and lights. |
| F29 | Context menu Paste | done | Native responder paste restores a cut group and imports a copied group in another window. Rust linking rules retain existing models; expanded parent and selection inspected. |
| F30 | Context menu Paste as New | done | Native context prompt creates Fresh and two renamed descendants. Saved file asserts fresh node/light IDs, preserved coordinates and unique orders1…4. |
| F31 | Clipboard is cross-window | done |74-step native suite opens two independent editor windows, copies from the first and pastes into the second. Separate document IDs, shared source subtree IDs and exact geometry verified from saved packages. |
| F32 | Context menu Rename | done | Single prompt plus shared native pattern and Find/Replace sheets from VixenKit; Rust uniquifies. Native3-node rename returned NewName-1…3; FindReplace Cancel unchanged. Existing15 naming tests pass. |
| F33 | Context menu Delete | done | Native context action removes selected Source group and orphaned lights; saved graph confirms removal. Keyboard Delete instead retains the empty group (F24). |
| F146 | Layout tab context menu: items and order (audit addition) | done | All context actions including final Find/Replace Rename follow upstream order. |
| F34 | Tree drag and drop: rules | done | Production provider/Rust rules and31-step provider suite plus actual system rejection of root-to-child, incompatible group, Option-copy and kind-compatible descendant cycle. Before/after insertion hover succeeds in both same/other-parent cases; rejected graphs stay exact and clean. Physical cross-window/alias/mixed-selection boundaries remain outside this bounded acceptance. |
| F35 | Tree drag and drop: drop | done | Actual center and same/other-parent before/after drops verify release routing, order, destination selection, exact graph identity and dirty/Move undo registration. Command-reverse physically yields Target `[Pixel3,Pixel2,Pixel1]`. Prior31-step provider suite retains undo coverage;15-case native evidence in `prop-drag-foreground` and `prop-drag-edges`. |
| F36 | Tree drag start | done | Actual WindowServer drags in `target/prop-drag-foreground` verify a single source, dragging selected Pixel2 moves both selected Pixel1+2, and dragging unselected Pixel3 after that selection moves/selects only Pixel3. AX pre/post-selection and exact saved graphs independently checked; no selection/drop hooks. |
| F37 | Order tab | done | Prior58-step edit and27-step delegate suites verify rename/undo/delete/reverse and foreign-source guard. Actual single-row and selected-pair system drags now produce exact `[Pixel2,Pixel3,Pixel1]` order1…3 with unchanged graph/identities. Physical foreign-window/cross-list rejection is outside this bounded acceptance, covered by provider/delegate checks only. |
| F38 | Order tab: Reverse | done | Native Reverse swaps selected slots and renumbers;27-step delegate suite repeats after Delete leaves orders 2,3 and verifies safe contiguous results. Saved orders independently asserted; gap-safe difference recorded. |
| F39 | State tab | done | State tab hosts definition/item UI and authored canvas preview; inspected native state tab. |

## C. Custom Prop Editor: drawing canvas

| F | Feature | Status | Where / what is missing |
|---|---|---|---|
| F40 | Delete selected lights | done | Native 89-step posted canvas suite sends Delete through canvas responder, removes selected lights, saves remaining graph and restores with undo. Authored assignment cleanup remains covered by Rust tests. |
| F41 | Draw mode vs selection mode | done | Native segmented Draw/Select; Rust modes tested and screenshots inspected. |
| F42 | Add a light (draw mode) | done | Native pointer handling invokes tested add-light rules; harness drew/reopened synthetic prop. |
| F43 | Selection mode: click | done | Posted native pointer events verify plain selection, Command add/toggle, preserving selected group on re-click and empty-canvas clearing. Strict selected-light/tree assertions pass. |
| F44 | Selection mode: drag | done | Native 89-step suite verifies full-circle rubber band inclusion/exclusion, live tree/canvas selection, band removal and multi-light movement/undo with exact saved coordinates. Rust covers additive bands and unclamped dragging; immediate first-event update is documented as a native-input difference. |
| F45 | Resize and rotate adorner | done | Native resize and Command-snapped 90-degree rotation persist expected geometry and undo. Corrected Swift drawing to use top-left thumb origins and rotate bounds/handles about the Rust center; rotated screenshot inspected. Existing Rust tests cover aspect, edges, clamp and rotated hit testing. |
| F46 | Align, distribute, flip toolbar | done | Native 66-step menu suite exercises all 8 actions, checks saved coordinates and undo. Preserves original Left/Right and Top/Bottom first-selected behavior. Menus disabled without selected lights. |
| F47 | Light size toolbar | done | Native 22-step posted-button suite verifies a two-light node increases 5→7, a neighboring single node 5→6, Match equalizes to 7 and Decrease steps per light again. Saved element/light sizes asserted; disabled controls screenshot inspected. |
| F48 | Canvas drawing | done | Native 50,000-light fixture loaded/drawn; cached 582x545-point raster benchmark improved 76.2→36.9 ms median in debug by direct CGContext circles and cached fill state. Draw order/alpha preserved. This is a CPU raster measurement, not a release/compositor frame-rate claim. |
| F49 | Selection sync details | done | Canvas selection now requests ancestor expansion; native 89-step suite verifies a collapsed group opens and its row selects. Live rubber-band selection refreshes the tree without rebuilding row data; tree/order select lights, canvas reset clears them. Rust 103 suite pins all alias-parent expansion requests. |

## D. Custom Prop Editor: State tab (definitions authored on the prop)

| F | Feature | Status | Where / what is missing |
|---|---|---|---|
| F50 | Definition row | done | `PropStates.swift` name combo/buttons/menu and Rust selection; native state tab inspected. |
| F51 | Add definition | done | Rust default item/new definition naming and native prompt; native harness authored definition and verified empty assignment error. |
| F52 | Delete definition | done | Native Delete/Cancel followed by Delete/confirm preserves then removes the selected copied definition; original assignments remain intact. |
| F53 | Rename definition ("..." > Rename) | done | Native ellipsis menu, prefilled prompt, renamed selection and persisted definition verified in 103-step panel suite. |
| F54 | Copy definition ("..." > Copy) | done | Native ellipsis Copy prompt creates and selects the second definition with unchanged assignments/counts/colors; fresh ids also pinned by Rust test. |
| F55 | Description | done | Multiline native description bound through Rust revision; persisted in package model. |
| F56 | State items grid | done | Virtualized native Name/Color/Count grid; editable hex and visible native color wells. Native54 item steps and33 system-panel steps verify text/hex input, assignments, two successive panel colors and saved values. Active wells survive continuous updates; columns fit the sidebar. Cross-editor ownership and actual table double-action callbacks now pass24 focused checks, including distinct-color persistence, no duplicate undo and same-table native-well takeover; the private fixture suppresses presentation and supplies clicked-row metadata, so this adds no physical picker claim. See `docs/review/prop-color-native.md`. |
| F57 | Clicking a column header sorts and persists | done | Native table descriptors call Rust sort and preserve direction/indicator; ascending Name and ascending/descending Count order persisted in the54-step suite. Offscreen windows ignore header presses, so the harness sets the native header descriptor to exercise its normal data-source callback. |
| F58 | Item buttons | done | Native Copy, Up/Down, Add, Remove/Cancel and confirmed removal verified; copy preserves assignments and a new empty item produces validation until removed. Saved values independently asserted. |
| F59 | Canvas assignment (state mode) | done | Rust assignment toggle/full-circle band/command-remove; multiple selected rows read-only, never moves lights. Draw assigns only newly added element in same undo. Five regression tests and native assignment count0→1. |
| F60 | State preview on the canvas | done | Bulk Rust per-element average colors (ties-even), base color for unassigned; native screenshot verified. No per-light assignment scans. |
| F61 | Validation messages | done | Rust validation before save; native wrapping red list and Invalid State Definitions alert. Native empty assignment error cleared after assignment. |
| F62 | Assignment clean-up | done | Graph mutation prunes stale ids within same undo snapshot; assignment dedup/order and removed owner restoration tested. |

## E. Custom Prop Editor: dialogs

| F | Feature | Status | Where / what is missing |
|---|---|---|---|
| F19 | Preferences dialog | done | Native ColorPicker preferences sheet, Defaults/Cancel/OK, uint size normalization; profile ModuleStore preserves unknown fields. Native size3→7 persisted and newly drawn light used7; two session tests. |
| F20a | Scale Background Image dialog | done | Native Pixels/Percent/aspect-lock sheet backed by `vixen-prop/scale.rs`. Three calculator tests; screenshot and native valid edit/apply/undo/redo verified. |
| F20b | Metadata tabs (pointer) | done | Native metadata picker presents General, Physical, Vendor and Notes; see F18a/F20c/F20d. |
| F20c | Physical metadata tab | done | Ordered string fields, descriptions as help, editable native Bulb Type combo with upstream suggestions, and Color Mode enum. All edits persisted and independently asserted from the package. |
| F20d | Vendor metadata tab | done | Ordered native Name, Contact, Website, Email and Phone fields; all edited and saved values asserted in the panel suite. |
| F145 | Shared text prompt (audit addition) | done | Native sheet preselects suggestion, Return confirms and Escape cancels; verb disabled while blank as requested. Prompt harness verified default replacement; fix recorded in DIFFERENCES. |

## F. Custom Prop Editor: Vendor Browser

| F | Feature | Status | Where / what is missing |
|---|---|---|---|
| F21 | Tools > Vendor Browser | done | `vixen-prop/vendor`, FFI `tools/vendor`, native `PropVendor`: injected transport, async progress/errors, partial inventory recovery, dirty replacement guard, xModel/wrapped-model/native prop download and metadata. Downloaded documents are untitled; no temp files. 74 core + 83 advanced native steps and strict saved-package assertions pass against loopback fixtures. |
| F21a | Vendor Inventory window | done | Native sheet with a 1:2 split, vendor picker, cached NSOutlineView categories/products, local-tested images/contact/links and Product Info/Model Options controls. Vendor switch clears product; empty email hidden; Cancel closes. Native outline selection, three direct Select buttons, wrapped-model picker and error paths verified. |

## G. Custom Prop Editor: xModel import

| F | Feature | Status | Where / what is missing |
|---|---|---|---|
| F63 | File routing | done | `vixen-prop/xmodel.rs`: case-sensitive elements, supported root/wrapper routing, unqualified attributes, unsupported/no-model errors; native malformed and unsupported error sheets preserve document. |
| F64 | `<models>` wrapper and "Select xModel" dialog | done | Rust candidate indexing/name disambiguation; native `PropXModelSheet` with first-supported selection, Model/Type table, OK/Cancel. Native chooses supported/unsupported rows and verifies cancellation. |
| F65 | Custom model parsing | done | `xmodel/geometry.rs`: dimensions, per-axis scales, away-from-zero rounding, compressed row/column precedence, duplicate last-wins, fallback warnings, blank holes and malformed rejection. Scoped tests and native fallback assertions. |
| F66 | Circle model parsing | done | Rust required fields, center/radius/direction, physical-layer wiring order, normalized coordinates and exact imported-Circles suppression. All supplied circle reference files round-trip; native 12-light two-ring geometry asserted. |
| F67 | Child elements | done | `xmodel/children.rs`: submodel ranges, faces/colors/components, NodeRange/SingleNode states, attribute insertion order, empty groups and unknown-order behavior. Tests plus native authored-state colors/counts. |
| F68 | Assembly into a prop | done | `xmodel/assemble.rs`: offset/minimum bounds, ascending lights, shared parent links, authored states plus legacy groups, source naming and generated circles. All supplied models survive package graph/state round-trips; native Stardust matches 1499 distinct orders and 12475 linked rows. |
| F69 | Vendor inventory XML | done | `vixen-prop/vendor/parse.rs`: exact child-element schema, recursive categories, multiple category links, uint rules, model defaults/type detection, URL handling and metadata mapping. Seven pure tests cover malformed inventory and partial transport failure. UTF16 decoding also backs local xModel files. |

## H. Wire diagram export (`App/Modeling`)

| F | Feature | Status | Where / what is missing |
|---|---|---|---|
| F20 | Export Wire Diagram | done | Front/Back menu and native shared options; Rust gathers each distinct light owner once, preserves repeated per-element order and truncates coordinates. SVG generated locally; no external app opened during tests. |
| F20e | Wire Diagram Export Options dialog | done | Shared `VixenKit/WireDiagramSheet.swift` used by Setup and Props; inspected native controls and accepted export. |
| F20f | Wire diagram SVG | done | Reuses tested `setup/wire_diagram.rs` SVG renderer:48 tests pass,1 performance test remains intentionally ignored. New prop gather regression passes; exported4-node SVG parsed with labels1…4. |

## I. Polygon Editor: host window and data model

Implementation: `crates/vixen-session/src/tools/polygon{.rs,/morph.rs,/interaction.rs}`, FFI `tools/polygon.rs`, and native `VixenTools/PolygonEditor.swift` / `PolygonCanvas.swift`. **12 scoped Rust tests pass**. Native sample suite `target/resume-ui/polygon` passes **44 steps**; actual Morph effect-row suite `polygon-effect` passes **42 steps**, then **10** fresh-process reload steps. `polygon-effect/out/assertions.txt` verifies unchanged Cancel, one undo, undo/redo, stable shape ID/geometry and saved label. Screenshots of FreeForm, ellipse, TimeBased and actual Morph were inspected. The expanded `polygon-gestures` suite passes **87 native steps**; its `check.py` validates numeric rejection, insertion, overlapping multi-shape lasso/move, every tested transform, rotated ellipse corner exclusion and time-marker quantization. `out/assertions.txt` records the result. All eight resize handles also have a parameterized Rust regression.

| F | Feature | Status | Where / what is missing |
|---|---|---|---|
| F70 | Polygon Editor window | done | Native owning-window sheet; canvas and window fit the owning display with the source 70% rule. Native control minimum sizes documented; updated large-window screenshots inspected. |
| F71 | Coordinate mapping | done | Window-local Rust scales, singleton denominator and margin outline; fractional grid/finish tests. |
| F72 | Results | done | `Editor.finish` copies once, snaps polygons/lines with ties-even rounding, preserves ellipse fractions and stable IDs. Native saved .tim reload verified. |
| F73 | Polygon data model | done | Typed draft shapes retain ID/label/fill/angle/start side and original Params for unknown geometry fields; renderer descriptors remain shared. |
| F73a | Who opens the editor, and with which capabilities (Morph) | done | EffectEditorHooks → openPolygonEditor → one sequence commit. Three Morph modes, ID-matched settings preservation, stable time sorting; no-op/cancel/deleted-effect tests and native undo/save/reload. |
| F74 | Toolbar | done | Native toolbar, conversion, clipboard and naming verified. Draft names match source selection: first polygon is deselected, restarted/subsequent polygons and lines are nameable. 14 Rust tests plus 62 native steps pin completion, discard/restart, stable shapes and time-frame naming. |
| F75 | Drawing a polygon | done | Rust click/ghost/close state, continuous FreeForm drawing, TimeBased returns to Select; minimum-three correction documented. |
| F76 | Drawing a line | done | Two clicks complete; repeated FreeForm drawing test and TimeBased native two-click workflow. |
| F77 | Drawing an ellipse | done | 20% smaller-dimension default, clamped corners, time-frame attachment; converted ellipse inspected natively. |
| F78 | Adding a point to a polygon | done | Rust distance-to-segment insertion; native AppKit event suite inserts a fifth point at the source segment and validates rows/geometry. |
| F79 | Selection mode: click | done | Native point/centre selection and overlapping-shape hit priority verified; ellipse corner clicks leave selection empty and centre selects all corners. |
| F148 | Polygon canvas cursors (audit addition) | done | Native arrow/crosshair/hand/add-copy cursor follows Rust hit tests; time marker horizontal resize cursor. |
| F80 | Moving a point | done | Ghost and neighbor lines, clamped mouse-up commit; test pins source point unchanged until release. |
| F81 | Lasso | done | Native lasso selects one full polygon and points across two shapes; both shapes move together with unchanged IDs and the expected exact offset. |
| F82 | Resize, move and rotate adorner (`IResizeable`) | done | Rust bounded transforms, shift scaling, refused rotation and ellipse angles; all eight resize handles tested, native polygon and rotated ellipse scaling/move/rotation verified. |
| F83 | Cut, Copy, Paste, Delete | done | Private NSPasteboard JSON, new pasted IDs, capability guards; native copy/paste/delete and model tests. Added canvas ⌘X/⌘C/⌘V documented. |
| F84 | Delete key | done | Whole shapes when permitted, otherwise polygon points down to triangle; line/ellipse point deletion skipped. Backend regression pins triangle floor. |
| F84a | Convert to Polygon, Line, Ellipse | done | Source default geometries and capability gates; native ellipse conversion and ID-preserving surrounding shapes. First-existing-line bug corrected. |
| F84b | Toggle Start Side | done | Point-list rotation, ellipse start-side modulo four, line endpoint swap/deselect; native ellipse toggle inspected and model rotation test. |
| F84c | Toggle labels | done | Native toggle and shape/point labels; inspected ellipse screenshot. |
| F84d | OK and Cancel | done | Cloned draft, one accepted sequence undo, cancel/no-op unchanged; 42-step actual Morph workflow plus 10-step fresh-process reload assertions. |
| F84e | Time bar (Morph TimeBased mode) | done | Frame add/clone/delete/selection and pixel-normalized drag sorting implemented. Tests pin cloning after deselection and integer pixel timing; native marker drags verify exact quantized values. |
| F85 | Polygon shape rules | done | Closed shape geometry, wipe restriction, start-side rotation, point insertion/deletion; too-small closure prevented as documented. |
| F86 | Line shape rules | done | Line geometry/endpoints/midpoint and Green loaded/converted/pasted versus Lime drawn/toggled starts; dedicated marker regression. |
| F87 | Ellipse shape rules | done | Unrotated scale then rerotate, computed centre/dimensions and start-side ordering; non-rounded rotated round-trip tested. Integer-division collapse fixed. |
| F88 | Point and marker colours | done | Exact DodgerBlue/HotPink, lime start sides, initial Green line marker, charcoal time markers and light-gray ghost/grid; selected point color takes precedence. |
| F89 | Polygon resize adorner | done | Eight handles plus move/rotate, rotated ellipse frame, zero-size denominators and invalid-rotation cancellation. Parameterized handle regressions and native rotated ellipse transformations pass. |
| F90 | Polygon canvas | done | Single cached native canvas draws grid/ticks/shapes/labels/ghosts/outline/adorner; Rust handles all geometry and hit testing. Updated screenshots inspected. |
| F91 | Side panel | done | Native name field updates draft per keystroke; virtualized NSTableView with validation and read-only ellipse points. Name entry and corrected column layout verified natively. |
| F92 | Point grid coordinate conversion | done | Rust per-window 1-based coordinates, Y flip and ties-even display. |
| F93 | Time bar view | done | Native 40-point marker/ruler canvas and previous/next/Add Frame/Delete Frame controls; inspected native TimeBased view. |
| F92a | Grid value mapping | done | Fractional input, finite/range checks and source validation messages; Rust tests pin Y flip and final snapping. |
| F93a | Time bar ruler | done | Ten major intervals, ten subdivisions each, 15/10-point ticks with edge margins. |

## J. Intelligent Fixture Wizard (moving head wizard)

| F | Feature | Status | Where / what is missing |
|---|---|---|---|
| F94 | Wizard window | done | `VixenSetup/FixtureWizard` native sidebar, navigation, help and Finish; 58-step private-profile harness passed. |
| F95 | Bookend moving heads | done | `VixenKit/MovingHeadView` uses shared GPU volumes, page colors and bookend angles. Required-Metal demo render and sweep test passes; saved GPU image inspected. AppKit snapshots omit Metal content. |
| F96 | Page Select Profile | done | Existing/new selection and profile fields; native Create New, all pages, Finish, actual overwrite Cancel/Replace Profile verified. Decline preserves saved bytes; acceptance writes the requested profile and creates Test Head1. `target/resume-ui/specialized-final` (99 steps). |
| F97 | Page Edit Functions | done | Shared `VixenDialogs/FixtureFunctionsView`; Rust draft validation blocks leaving invalid details. Native rotation/index errors and corrected edits verified. |
| F98 | Page Edit Channels | done | Shared `FixtureChannelsView`, no profile/cog fields in wizard; Rust blocks Back/Next on errors. Native channel creation/assignment verified. |
| F99 | Page Color Support | done | Native color support page, source defaults in session fixture wizard; native walkthrough plus Rust color/filter tests. |
| F100 | Page Automation | done | Native automation choices and dependency rules; Rust tests. User choices survive revisits until their dependencies change (audited spec correction). |
| F101 | Page Dimming Curves | done | Existing dimming page and curve editor integration; per-color selection clears when support no longer permits it. Rust tests and native walkthrough. |
| F102 | Page Grouping | done | Existing grouping controls and native tree preview; Finish created Test Head1 under Fixtures. Revisit preserves answers. |
| F103 | Page Summary | done | Native summary screenshot inspected; same titles/text, Finish runs real setup generation. |
| F104 | Finish: create fixture elements | done | `session/setup/fixture.rs`; saves library then creates elements, with FFI validation before either. Native Finish and saved library verified; backend save/reload tests. |
| F105 | Per fixture node | done | `create_fixture_node`; fixture property, colors, location and ordering preserved by existing fixture generation tests. |
| F106 | Per function dispatch | done | Existing tested per-function generation in `setup/fixture.rs`; shared editors now supply full validated profiles. |
| F107 | Colour mixing chain | done | Existing RGB/RGBW/coarse/fine/filter-chain tests in `setup/fixture.rs`; native UI edits preserve the same profile model. |

## K. Intelligent Fixture property

| F | Feature | Status | Where / what is missing |
|---|---|---|---|
| F108 | Property | done | `PropertySetupSheet` now opens `FixturePropertySheet`; dialog-world apply keeps individual property instance IDs and module-store metadata. Native save to two selected fixtures and Rust round-trip verified. |

## L. Fixture Property Editor (profile channels window)

| F | Feature | Status | Where / what is missing |
|---|---|---|---|
| F109 | Window | done | `VixenDialogs/FixtureEditor.swift`; native title/help/OK/Cancel and validation, library save on OK. 50-step property/functions/save workflow passed. |
| F110 | Profile fields | done | Profile/manufacturer/revision/created-by controls inspected; Load library workflow passed native harness. |
| F111 | Channels grid | done | Native channels table, editable names/function pickers, cog, add/remove; Rust normalization/renumbering. Incomplete assignment is atomic as audited spec requests; existing incomplete functions remain visible. |
| F112 | Load Fixture Specification dialog | done | Native library list selects first; empty list disables OK. Load replaced a changed draft with the saved profile; native snapshot inspected. |
| F113 | Cog button (Edit function) | done | Cog/shared function sheet uses a detached draft; OK replaces definitions and reconciles deleted names to None. Native child-sheet save verified, Rust delete/assignment test. |
| F113a | `CanSave` aggregate | done | `setup/fixture_editor.rs` returns aggregate errors; native OK/Next gating and message presentation verified. |

## M. Function Type editor ("Edit Functions")

| F | Feature | Status | Where / what is missing |
|---|---|---|---|
| F114 | Window | done | Native Edit Functions sheet with shared split view and help/OK/Cancel. Native wheel, indexed and rotation pages inspected; width expanded to keep wheel names readable. |
| F115 | Functions list (left group "Functions", width 500) | done | Functions table/types/tags/legend, add/remove/focus, None sentinel, unique names/rotation tags. Rust uses a fixed bounded timeline-color palette (documented difference). Native added Color Wheel saved. |
| F116 | Selection gate | done | Rust `ProfileEditor::try_select`; native invalid rotation and index End Value kept selection, correction released gate. |
| F117 | Details panel (right group, title "{name} Details" or "<Function> Details") | done | Rust panel dispatch by type/tag; native Pan, Gobo, Shutter, Zoom and Color Wheel panels inspected. Associated prism selector implemented. |
| F118 | Indexed panel | done | See F118a. |
| F118a | Indexed panel | done | Draft index rows, DMX/range/name checks, Gobo image choices/import and associated Prism selector. Native blank/bad End Value correction and Rust inclusive-overlap tests; file copy path remains local. |
| F119 | Color Wheel panel | done | Native color wheel table and system picker, half-stop inherited colors, exclusion/curve controls. Rust recomputation and End Value validation; native three-entry red/split/blue wheel persisted. |
| F120 | Pan/Tilt panel | done | See F120a. |
| F121 | Zoom panel | done | See F121a. |
| F120a | Pan/Tilt panel | done | Native rotation fields and shared GPU animation driven by display link; fixed source sweep, default 0/360 for new functions. Required-GPU rendering test and native validation/save (10–540) passed. |
| F121a | Zoom panel | done | Native Narrow To Wide / Wide To Narrow radios; typed Rust value, native changed Zoom value persisted. |
| F122 | Grid cell behaviour | done | Direct-edit native Table controls; invalid rows remain editable and Rust guards row transitions. Added function/index names receive focus. |
| F123 | Delete sentinel values | omitted | WPF delete sentinels have no purpose in native tables; removal mutates the Rust draft directly. |

## N. State property and State Mapper

| F | Feature | Status | Where / what is missing |
|---|---|---|---|
| F124 | State property | other area | `session/dialogs/state.rs`, `ffi/dialogs/state.rs`; checked in property/filter/controller parity and dialogs review. |
| F125 | Setup helper "State Mapping" | other area | State Mapping helper and normalization; existing dialogs implementation and native helper workflow. |
| F126 | Normalization and defaults | other area | StateDraft normalization/defaults tested against C#; `docs/review/dialogs.md` entries 37–40. |
| F127 | State Mapper window | other area | Native `StateMapperSheet`; earlier 48-step assignment workflow passed, screenshots inspected. |
| F128 | Definition row | other area | Definition picker/name/description and copy/delete tested in existing dialogs implementation. |
| F129 | Preview toggle | done | Native Preview/Remove/Cancel/Delete actions observed through the actual engine frame sink: 16 white elements persist, canceled delete preserves output, same-index replacement and final-item deletion clear output, mapper Cancel leaves it dark. `target/resume-ui/specialized-final/state-delete` (48 steps); no physical-light or Metal-presentation claim. |
| F130 | State Items grid | other area | Native state items table with stable IDs; row replacement now refreshes assignments even at the same index. |
| F131 | Colour picking | other area | System color picker in State Mapper; native sheet inspected, backend color persistence tested. |
| F132 | Item buttons | other area | Add/copy/remove/reorder in existing StateDraft; native assignment workflow and Rust tests. |
| F133 | Assigned Elements pane | other area | Assignment NSOutlineView preserves enabled selected rows across Toggle Selected; fixed and verified in dialogs review D4. |
| F134 | State definition name prompt | other area | State definition naming prompt and normalization; Rust naming tests and native prompt. |
| F135 | Mapper validation messages | other area | StateDraft issues and global blocking validation match C#; dialogs review entry 38. |
| F136 | Dead code | omitted | Unused WPF-only dead code is not surfaced as a native command. |
| F137 | Who uses State | other area | State effects, mapper and preview/custom-prop import share persisted state IDs; authored import remapping verified in preview pass 3. |

## O. Moving head graphics (`Editor/FixtureGraphics`)

| F | Feature | Status | Where / what is missing |
|---|---|---|---|
| F138 | Settings | done | Shared renderer `MovingHeadParams`, session FixtureSet/worker; cached equality replaces source redundant updates. Test coverage in preview and fixture editor suites. |
| F139 | Geometry constants | done | `vixen-render/src/fixture.rs` shares all body/support/housing proportions; pure geometry tests and required-GPU images inspected. |
| F140 | OpenGL model | done | Shared OpenGL-derived model, two-color beam, focus/length and black stub; native live fixtures and required-GPU tests. |
| F141 | Volumes | done | Shared cylinder/box volumes preserve original divisor-19 seam and cap-center quirk. Source behavior documented in mesh code. |
| F142 | Shaders and draw order | done | Shared GPU shaders, body dimming and depth, cached mesh; each beam half draws once per audited spec. 50 headless/16 live/1 idle renderer tests passed on Metal. |
| F143 | WPF model | done | Editor local perspective uses EditorTopPlusBottom focus formula and shared volumes. Owner chose OpenGL +180° bottom tilt consistently; no separate WPF engine. |
| F144 | Who depends on the graphics | done | Live preview, designer thumbnails, wizard decorations and rotation editor all use vixen-render fixture geometry; native wrapper in VixenKit. |

## Fixture editor verification (2026-11-11)

- Shared backend: 75 fixture/editor tests pass, including required Metal rendering, exact whitespace parsing, invalid draft gates, split wheel colors, property module-store round-trip and original fixture generation tests. Log: `target/fixture-editor-all-tests.log`.
- Native wizard: 58 steps, zero failures, snapshots inspected; invalid rotation/index fields blocked selection until corrected, then a channel was added and Finish created a fixture. `target/fixture-editor-nov11/wizard-final`.
- Native property editor: 50 steps, zero failures; Gobo repair, Pan/Zoom edits, three wheel rows and channel assignment saved to both selected fixture nodes and the profile library. XML verified. `target/fixture-editor-nov11/property`. Table cells/add actions use the same model callbacks through harness hooks because SwiftUI icon buttons are not exposed by its in-process accessibility tree.
- Final library-load/nested-Cancel workflow: 28 steps, zero failures; reloaded profile fields and Cancel retained all three profile/library XML files byte-for-byte. `target/fixture-editor-nov11/cancel`.
- Final wheel layout: 22 steps, zero failures; inspected 1320-point sheet shows all wheel columns. `target/fixture-editor-nov11/layout`.
- Wizard shared dimming curve editor: 47 steps, zero failures; Cancel retained the ramp, Return accepted a reversed curve, reopening showed it, and Finish/Display Setup OK saved `(0,100),(100,0)` in DimmingCurveData. `target/fixture-editor-nov11/curve-key`. Earlier two mouse-label attempts missed the short OK label; final run uses the native default Return shortcut.
- Final overwrite/State-preview closure: **147 native steps pass** in `target/resume-ui/specialized-final` (51 decline, 48 replace, 48 state deletion); `check.py` verifies saved XML, unchanged copied SystemData and actual engine frames. Create New and the real overwrite prompt are exercised; test hooks supply fields because offscreen native text focus is absent. The State test keeps selection index 0 while deleting its original item, proving the refresh cannot rely only on selection change. The observer is installed only under `VIXEN_UI_TEST`; no physical output or onscreen Metal claim. No production behavior change was needed.
