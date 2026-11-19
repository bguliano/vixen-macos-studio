# MCP integration handoff

## What the UI branch should consume

The shared Rust display ownership/undo refactor is implemented. AppCore owns StudioDisplay. The redesigned non-modal pane must call studioDisplayCommand and fetch studioDisplaySnapshot/history, rather than hold openDisplaySetup's legacy private working copy. The legacy API remains compatible during migration, but mixing both writers invalidates Studio snapshot history rather than overwriting newer state.

Migration status: step 1 is done. Display Setup is the show window's Display section (`StudioDisplayPane` in VixenKit, registered by VixenSetup), still on the legacy working copy with an Apply Changes / Discard Changes bar and a prompt when leaving the section with unapplied edits. Next: move each part to studioDisplayCommand with live undo, then show an in-place notice when Live Output blocks an edit. Until then, an open pane keeps `AppModel.isBusy` set, as the window did.

Use StudioAgentActivity.observe(core:) for the toolbar, and studioAgentSnapshot on reconnect/event gaps. It tracks real connected clients, active tools and bounded action outcomes. Callback delivery is marshalled to MainActor. For sequence windows, use studioOpenSequenceSession(sequenceId:) for an unsaved agent document. Opening a saved agent sequence through ordinary openSequence reuses its shared session. documentChanged carries documentId; refresh the relevant view from that shared session. Display edits publish displayChanged; preview edits also set preview_needs_reload.

Wire the local setting to studioSetAllowAgentsLiveOutput. It defaults OFF each app launch; there is deliberately no MCP setter. Revocation stops agent-started controllers/test output. Startup is one explicitly confirmed network controller at a time; execution must first be opened locally. Tests never enable real hardware.

Full FFI/event/command contract and explicit limitations: MCP.md. Current tool inventory: MCP-TOOLS.md (71 tools when generated). Client registration: MCP-CLIENTS.md.

## Verified work

- M0 design committed before protocol work; draft PR #1 targets studio from studio-mcp.
- M1 read-only stdio foundation: initialize, tools/resources/prompts, seven initial tools, profile locking and real-show tests.
- M2: shared display/preview/show transactions and Agent-labelled undo; native controller configuration/patching; sequence effects/parameters/length/layers/marks/transport using the shared SequenceSession; in-app socket and callback/activity adapter; signed bundle helper.
- M3: real media beat/bar analysis, typed parameter read/write, CPU preview inspection frames, local FSEQ export; scripted stdio authoring of a 30000ms real-music sequence with 20 beat-synced effects, rendered schedule, TIM/FSEQ/PNG outputs, show/disabled schedule drafts, native preview prop creation/linking, confirmed profile save.

The preview PNG was inspected: actual illuminated strands at their native preview coordinates on the intentionally plain black inspection background. It is not a mock UI screenshot or a claim of GPU/background/fixture rendering parity.

## Test evidence

Commands run with VIXEN_MUTE=1 and CARGO_BUILD_JOBS=2, serialized:

- cargo test -p studio-mcp: 6 passing integration tests (4 stdio/locking/protocol, 1 socket/live undo/shutdown, 1 real-song authoring/export/preview/show/prop end-to-end).
- cargo test -p vixen-session studio_ --lib: 5 passing shared schema, permission/mute, display transaction, controller/template/save, and sequence undo/parameter tests; the subsequently added studio_paths test separately passed (1).
- cargo test -p vixen-io file::tests --lib: 11 passing atomic-save/backup regressions after exclusive temp creation hardening.
- Initial admitted admin+setup application build passed, including the real UniFFI callback and StudioAgent target. Signed bundled helper initialize + 18-tool catalog smoke passed at that M2 point. A final expanded build and packaged-app smoke are recorded below when complete.

Local logs/artifacts are under target/ and are not committed. Real-song fixtures come from the copied show; Media is read-only. No tests played audio, started controllers, uploaded FPP data or activated a schedule.

## Marked changes to vanilla files

All new implementation is in Studio-only crates/files except these narrow marked hooks:

| File | Purpose |
|---|---|
| Cargo.toml | Add studio-mcp workspace member |
| Cargo.lock | Generated local MCP/FFI dependency entries; comments must be restored after Cargo regenerates |
| app/Package.swift | Build StudioAgent and expose it to VixenApp |
| crates/vixen-ffi/Cargo.toml | studio-mcp and serde_json dependencies |
| crates/vixen-ffi/src/lib.rs | Register studio_agent exports; trivial merge with coordinator's studio_library registration |
| crates/vixen-ffi/src/admin.rs | Start MCP on the existing app core; report startup failure through logs |
| crates/vixen-ffi/src/sequence.rs | Shared-session wrapper and reuse an already-open sequence path |
| crates/vixen-io/src/lock.rs | Recognize the lowercase headless bridge as a live profile-lock owner |
| crates/vixen-io/src/file.rs | Exclusive save temporary creation; cleanup only owned temps (suitable upstream hardening) |
| crates/vixen-session/src/app.rs | One Studio command service per AppCore |
| crates/vixen-session/src/boot.rs | Stop commands/server before profile-lock release |
| crates/vixen-session/src/lib.rs | Register Studio-only command/codec/diagnostic/path modules |
| crates/vixen-session/src/setup/commit.rs | Materialize a validated candidate without save or hardware side effects |
| crates/vixen-session/src/setup/mod.rs | Module-only dirty bit for preview/show/schedule transactions |
| crates/vixen-session/src/preview.rs | Reuse native light-color conversion for inspection PNGs |
| crates/vixen-session/src/undo.rs | Native undo variant/description for sequence length/layer configuration |
| crates/vixen-session/src/sequence.rs | Apply/invert that native undo action |
| scripts/build_app.sh | Build, bundle and sign vixen-studio-mcp in Contents/Helpers |

STUDIO.md documents the engine rationale; studio/markers.lock records intended hashes. An inherited .gitattributes marker missing from the starting inventory was recorded in a separate early commit; its file content was not changed. No existing Swift views were restyled, no merge into studio was performed, and no push to upstream was made.

## Open issues / limits

Read MCP.md's Known limits before calling this exhaustive parity. Important remaining boundaries: no automatic lyrics alignment, heuristic onset/section estimates and fixed 4/4 bars, selected unsupported parameter codecs/specialist controller dialogs, inspection-only PNG rasterization, disabled schedule drafts requiring local review, no MCP cancellation/tasks/HTTP, memory-heavy full display snapshots, and coordinator-owned final UI/reload wiring. Real hardware operations remain untested by design. These are explicit limits, not placeholder tools or silent fallback paths.

## Final packaged-app verification

The expanded admitted admin+setup build succeeded with the new FFI/activity model, followed by the packaged-app smoke harness: liveSession attachment, actual in-app create/undo, default-off output and normal-shutdown socket cleanup all PASS. The initial smoke used the generic harness quit action, which calls exit directly and deliberately bypasses normal shutdown; the corrected smoke closes the Administration window and exercises the normal application termination path. Its failed-run test socket was cleaned up only after verifying its process had exited.

After that successful build, two protocol metadata corrections were made: live playback/seek safety annotations and initialize guidance. A repackage attempt was rejected by the configured build-admission gate before compilation. Source is current; the existing local bundle is the immediately preceding verified build. Do not describe the final metadata-only edits as repackaged until admission permits another build. No gate was bypassed.

After Cargo rewrites Cargo.lock, run python3 scripts/studio_mark_mcp_lock.py before marker checking/recording. It uses a full native file read, validates semantic TOML equality before writing, and restores only the Studio comments atomically. Do not reconstruct a long generated lockfile from a bounded/truncated tool read. Missing-marker restoration and repeated idempotent runs were tested; the dependency graph remains 328 packages.
