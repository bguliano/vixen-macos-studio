# Vixen Studio MCP contract

## Design and implementation milestones
M0 defines the contract; features below are planned until listed as implemented.

The bridge `vixen-studio-mcp --profile PATH` speaks newline-delimited MCP JSON-RPC on stdin/stdout (logs only on stderr). Prefer the in-app Unix socket under `~/Library/Application Support/Vixen Studio/` (directory 0700, socket 0600). No TCP listener or remote exposure. A profile identity handshake must prevent attaching to the wrong show. Headless fallback must acquire the same exclusive profile lock as the app; no fallback on a busy or inaccessible socket. Headless is always muted, with live hardware disabled.

## Shared commands and events
`vixen-session` owns validation, mutations, undo and render scheduling. The MCP crate is transport/schema adaptation only. Agent requests must operate on the same Arc-backed sequence/setup sessions as Swift; independently reopening an active document is forbidden. Each successful edit records one atomic undo action labelled `… (Agent)`. Save and destructive commands require `confirm: true`; ordinary non-destructive edits do not. A failed command must not publish a revision or partial edit.

The Studio-only FFI event interface carries monotonically increasing revision, kind (`clientConnected`, `clientDisconnected`, `commandStarted`, `commandFinished`, `documentChanged`, `displayChanged`, `outputChanged`), client ID, command, document ID, summary and success. UI callbacks marshal to MainActor; callbacks never execute with an engine lock held. Clients obtain a snapshot on attachment and after a revision gap. Activity is bounded; no audio paths or tool arguments in logs by default. `app/Sources/StudioAgent` exposes an @Observable MainActor activity model (connected clients, current tool, recent actions). UI owns toolbar styling.

## Capability inventory
- profile: status, app info, profile selection, sequences/media, explicit save
- display: elements/groups CRUD, templates, colors/properties, wire order
- controllers: CRUD/types, outputs/universes, patch/unpatch/report
- live: separate start/stop/test commands; both explicit confirmation and local setting required
- preview: props, linking, frame PNG rendering
- sequence: create/open/length/audio, effects CRUD/parameters/catalog, layers, copy/paste, marks, undo/redo, render status, muted transport
- music: tempo/beats/bars/onsets/energy; beat marks
- shows/schedules: list and edit
- diagnostics: patches, overlapping universes, missing media, incompatible colors, logs and controller stats
- export: explicit confirmed fseq output

Tools have closed object JSON schemas, stable IDs, clear units (milliseconds unless specified), bounded lists and readOnly/destructive annotations. Advertise only executable tools, not placeholders. MCP resources expose this contract and profile summaries; workflow prompts describe inspect → analyze → construct → render → validate → confirmed save/export.

## Safety
Allow agents to control live output defaults OFF, is set only by local UI, and cannot be enabled by MCP. VIXEN_MUTE=1 overrides all settings. Neither analysis nor playback starts controllers. All writes stay within the selected profile or an explicitly approved export destination; canonicalize paths and reject symlink escapes (especially Media). Original show is read-only and never opened for writes. Destructive edits/overwrites require explicit confirmation. Socket peers are local same-user processes; file permissions are the trust boundary, not protection against other same-user malware. Bound messages, responses and work; disconnect releases client activity state.

## UI integration requirements
Do not fork sessions for MCP. App starts/stops the server with its core lifecycle and exposes the shared activity/event adapter. Existing UI session ownership and undo handling must be unified before live mutation tools are advertised. UI may render event snapshots without implementing business rules.

## Implemented scope (M1 foundation)
The initial implementation is a locked, read-only headless snapshot with seven tools, resources and a workflow prompt. It deliberately does not advertise mutations, socket attachment, analysis, render/export or playback. Existing session APIs interpret display/patch data. Stdio has 1 MiB input and 8 MiB response limits, strict tool arguments and 500-item pages. No app core is booted and no media is decoded.

Integration prerequisite discovered: DisplaySetupSession privately owns a RwLock<SetupSession>, while open sequencers register Arc<Mutex<SequenceSession>> with AppCore. Display edits lack a shared session-level undo stack. M2 must unify setup ownership and add shared display undo first; an agent-only working copy is not live parity. Sequence actions can use UndoAction::Group to label shared undo entries with (Agent). The profile-lock recognizer must recognize the lowercase bridge or the app treats its live lock as stale.

Preflight: no existing MCP dependency in this repository. The narrow synchronous read-only adapter uses existing serde_json instead of introducing an async runtime. Before concurrent sessions/cancellation/HTTP, evaluate the maintained official Rust MCP SDK (rmcp) and migrate transport rather than expanding a bespoke stack.

StudioAgentActivity is a standalone @Observable MainActor adapter with bounded snapshot replacement. It is not yet registered in Package.swift or connected to FFI; do not show a connected indicator until a real server snapshot arrives. No existing views were changed.

## M2 live display API (2026-11-18, supersedes M1-only limitations above)

The profile now owns StudioDisplay in AppCore. Both MCP and Swift call the same validated command service, which uses the existing SetupSession algorithms in an isolated candidate and publishes the completed SystemData with AppCore::apply_display. A failed command cannot partially mutate the live display. Undo/redo retain up to 20 profile snapshots; UI and agent commands interleave on one stack and agent entries end with '(Agent)'. Stable UUIDs cross this API, never temporary arena row paths. Non-modal panes must use this service, not hold an old setup working copy.

Swift-facing VixenCore API (generated Swift lower-camel names):
- studioDisplayCommand(command:argumentsJson:) throws -> JSON result {changed, revision, result}. This is the UI origin; agent origin is supplied internally by the MCP adapter, never an argument.
- studioDisplayCommandCatalog() -> JSON array of command schemas.
- studioDisplaySnapshot() -> JSON {revision,elements,controllers}; element rows include id, name, elementId, child UUIDs. Fetch in bulk off MainActor.
- studioDisplayHistory() -> JSON {revision,undo,redo}; descriptions are newest first.
- studioObserveDisplay(observer:) / studioStopObservingDisplay(). One observer per core; replace on rebinding. Callback is off MainActor and outside service/engine locks.
- studioDisplayEvents(afterRevision:) -> bounded recent events. Event revision is its own monotonic counter, NOT the display revision used for optimistic edits.
- studioStartMcp() throws -> explicitly retry/report local server startup. Normal VixenCore.start starts it automatically; lower-level test VixenCore.open does not.

Commands currently wired: element_create, element_rename, element_delete, element_group, controller_create, controller_rename, controller_delete, patch_elements, unpatch_elements, display_undo, display_redo. Every command accepts optional expectedRevision from the display snapshot; stale values fail before mutation. Delete/unpatch requires confirm:true. Newly created controllers are disabled. Agents cannot edit display while non-muted execution is open, because applying a graph could itself restart hardware; close execution locally first. No live-output tool is advertised yet.

After successful publication: displayChanged event. The server also emits clientConnected/clientDisconnected and commandStarted/commandFinished/commandFailed. For client events, command contains an opaque connection ID; for command events it contains only the tool name, never arguments. StudioAgentActivity.observe(core:) forwards actual callbacks onto MainActor and retains at most 100 actions. The pane should refresh its display snapshot on displayChanged; do not confuse event counters with expectedRevision. No views were restyled.

Legacy openDisplaySetup/commit/discard remains unchanged during migration. Do not mix a legacy modal working copy with the new live pane: legacy publication invalidates the live history on the next command rather than undoing unrelated external edits. Full parity applies to the new service API; the old modal APIs are compatibility-only until the UI switch.

The bridge now prefers the per-profile Unix socket (0600, directory 0700), validates canonical profile identity, and falls back to locked headless mode only if the socket does not exist. Existing broken/stale/untrusted socket endpoints fail closed. Max 8 connections; idle read timeout 120 seconds, write timeout 30 seconds. App shutdown first rejects further commands and drops the listener before releasing its profile lock. The bridge is built/signed into Contents/Helpers by the marked build script.

Still open: sequence/music/preview/export commands; controller network setup and templates/colors/properties; persisted live-output setting and gated operations. Do not present these as completed based on the transport work.
