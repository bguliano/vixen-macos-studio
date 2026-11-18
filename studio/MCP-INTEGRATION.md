# Integration handoff

The protocol foundation does not satisfy full MCP parity yet. Do not advertise full show authoring in the UI.

## Shared-session work before mutations

- Current DisplaySetupSession owns a private RwLock<SetupSession> and publishes through apply_live/commit. Move ownership into an AppCore-owned Studio command service; both UI and MCP must borrow it. Coordinate this with the simultaneous UI redesign before altering setup.rs.
- Display undo is not in SequenceSession's UndoStack. Add a session-level display command transaction and shared undo history; avoid a separate MCP-only snapshot stack. Disk saves remain explicit.
- Open sequences already register shared Arc<Mutex<SequenceSession>> handles in AppCore. Resolve MCP IDs through that registry; never reopen an existing sequence from disk. Add one UndoAction::Group with an Agent label per request.
- Publish callbacks outside engine/session locks. Snapshot replacement is already represented by StudioAgentActivity, but no server or FFI callback currently drives it. Register the Swift target and wire the toolbar only after the callback exists.

## Transport follow-up

- Adopt/qualify rmcp before growing transport concurrency.
- Add same-user 0600 Unix socket, profile identity handshake, bounded clients and server lifetime owned by AppCore. Only use headless fallback when no app is serving the selected profile, never after an ambiguous connection failure.
- Bundle bridge in Contents/Helpers via marked build script edits. Current focused tests build the helper; app packaging is not changed.
- Add an AppCore-local persistent Allow agents to control live output preference defaulting false. MCP must not have a setter. All hardware operations require both preference and confirm, with mute overriding both.

## Remaining coverage

Display/controller edits, patching, preview props/frame PNG, sequences/effects/parameters/layers/marks, analysis, shows/schedules, fuller diagnostics, controller monitor and FPP export remain unimplemented. Test all writes on disposable copies; Media links are read-only. The requested 30-second music-synced MCP authoring/render end-to-end test has not run because authoring/render tools do not yet exist.

## Vanilla markers

- Cargo.toml: Studio-only MCP workspace member.
- Cargo.lock: generated local MCP dependency entry (Cargo may regenerate this; record/check after regeneration).
- crates/vixen-io/src/lock.rs: mcp-profile-lock recognizes lowercase bridge processes, preventing the app from treating an active bridge lock as stale.
- studio/markers.lock also records an inherited .gitattributes marker that was missing from the starting inventory.

No existing Swift views, build script or app lifecycle code were changed.

## Verification (2026-11-18)

VIXEN_MUTE=1 CARGO_BUILD_JOBS=2 cargo test -p studio-mcp:
4 passed; 0 failed; 0 ignored. Includes real target/show-copy stdio round trip of all seven tools, malformed protocol/bounded frame handling, existing-lock refusal and app recognition of a live lowercase bridge lock. No audio or output starts. Existing fx-pixel-simple unused-assignment warning remains unrelated.
Swift activity adapter passed swiftc -frontend -parse; it is not yet compiled as an integrated app target. No full app build or UI rendering claim.

## M2 progress update
The earlier shared-session blocker is resolved by AppCore-owned StudioDisplay and its new FFI command/snapshot/history/observer API. See the final M2 section of MCP.md for the contract to wire into the sidebar pane. The legacy modal API is intentionally unchanged; its private copy must not be used by the new pane. Live socket edit + UI undo + shutdown rejection test passes. App packaging and Swift callback build verification are in progress. M2 sequence editing and M3 remain open.

M2 display/transport verification: admitted admin+setup app build succeeded, including generated UniFFI callback and StudioAgent Swift target. Bundled signed helper initialize + 18-tool catalog smoke test passed. One live Unix socket integration test passes (actual shared-core edit, UI-path undo, shutdown rejection), alongside four stdio/lock tests. Remaining authoring coverage is still in progress; this is not a claim that all M2/M3 tools exist.
