# Implemented MCP tools

74 tools, generated from the tested bridge tools/list result. No placeholder tools.

| Tool | Read only | Description |
|---|---|---|
| studio_status | true | Profile identity, safety state and implementation capabilities. |
| recent_logs | true | Read up to limit lines from the newest profile log, bounded to 64 KiB; never follows a log symlink. |
| controller_monitor | true | Actual execution/frame/controller run statistics; no startup or polling side effects. |
| show_list | true | List shows and sequential items without running them. |
| schedule_list | true | List saved schedules, activation state and display revision. |
| preview_props | true | List native prop IDs and property metadata for every preview. Use returned revision for edits. |
| preview_list | true | List configured preview IDs and names. |
| controller_types | true | Available controller types with stable UUIDs. |
| display_elements | true | List element nodes with stable GUIDs and child GUIDs; paginated. |
| controller_list | true | List configured controllers and output counts. Does not start output. |
| sequence_list | true | List sequence filenames in the selected profile. |
| media_list | true | List media filenames, read-only; does not decode or play audio. |
| effect_catalog | true | List registered effects and JSON parameter schemas. Unsupported native value kinds have a rejecting schema. |
| diagnostics_report | true | Report patches, controller/universe errors, and missing media/targets/native color corrections for open sequences. |
| profile_save | false | Save current live display configuration and module data. Explicit confirmation required; retains undo history. |
| element_template | false | Create a bounded arch, group, megatree or matrix using native template algorithms. count=segments/strings/rows; pixels=pixels per string/columns. |
| element_color | false | Set RGB or single-color handling on an element/group's leaves. |
| element_wire_order | false | Reverse the selected element/group row order using the native setup command. |
| controller_configure | false | Configure DDP or E1.31 unicast using an IPv4 literal. Does not connect or start output. E1.31 requires firstUniverse; universeSize defaults 510. |
| element_create | false | Create an element, optionally under a parent UUID. |
| element_rename | false | Rename an element by stable UUID. |
| element_delete | false | Delete an element link. Requires explicit confirmation. |
| element_group | false | Create a group linking existing element UUIDs. |
| controller_create | false | Create a disabled controller. Never starts hardware. |
| controller_rename | false | Rename a controller by UUID. |
| controller_delete | false | Delete a controller and its patches. Requires confirmation. |
| patch_elements | false | Patch unconnected element/group channels to unused controller outputs in tree order. startOutput is zero-based. |
| unpatch_elements | false | Remove element/group patch connections; retain filters. Requires confirmation. |
| display_undo | false | Undo the latest shared UI/agent display edit. |
| display_redo | false | Redo the latest shared UI/agent display edit. |
| show_create | false | Create a sequential show from local .tim filenames; no program-launch actions or playback. |
| show_rename | false | Rename a show by UUID. |
| show_delete | false | Delete an unreferenced show after confirmation. |
| schedule_create | false | Draft a DISABLED schedule. id is show UUID; start/end are local ISO date-times; days is seven booleans Sunday-first. Enable locally after review. |
| schedule_delete | false | Delete a schedule by index with explicit confirmation and snapshot revision. |
| preview_prop_create | false | Add a native preview prop at the default location; change geometry with preview_prop_set. |
| preview_prop_link | false | Link a preview prop to element node UUIDs using native linking rules. Requires snapshot expectedRevision. |
| preview_prop_set | false | Set a native preview property. Use metadata IDs from preview_props and the matching valueType. Requires snapshot expectedRevision. |
| effect_parameters | true | Read current typed effect parameter values and writable schemas. Native gradient stop positions are reported separately from the evenly-spaced write format. |
| sequence_length | false | Set length without truncating effects, with shared undo. |
| sequence_layers | true | List layers and mixing filters. |
| layer_create | false | Create a highest-value mixing layer with undo. |
| layer_rename | false | Rename a layer with undo. |
| layer_delete | false | Delete a non-default layer with confirmation and undo. |
| effect_layer | false | Assign an effect to a layer with undo. |
| effect_copy | false | Copy an effect and parameters to another target/time in one undo step. |
| sequence_play | false | Play muted in headless mode. A non-muted app requires local agent-output permission and confirm:true because sound/output may occur. |
| sequence_stop | false | Stop this sequence if it owns transport. |
| sequence_pause | false | Pause this sequence if it owns transport. |
| sequence_seek | false | Seek in milliseconds. Non-muted seeking needs local permission and confirm:true. |
| effect_set_parameter | false | Set one typed parameter through native setters, with undo. See effect_catalog schemas. File-path parameters are not accepted. |
| preview_frame | false | Render real preview light geometry at a sequence time to a PNG inspection frame. Omits background/fixture beams. Requires confirmed output filename. |
| music_analyze | true | Analyze actual profile media with Vixen's beat/bar tracker and per-band spectral-flux onsets; returns millisecond beats, bars, tempo, onsets with bar/16th slots and a per-bar slot-strength grid. Assumes 4/4; no playback. |
| sequence_create | false | Create a new in-memory sequence with optional profile audio; never plays sound. Its sequenceId changes to the file's after sequence_save (the old one keeps working). |
| sequence_open | false | Open a profile Sequence filename. sequenceId is derived from the file, so every connection gets the same ID. alreadyOpen:true means this Studio session already had the file open (in the app or by an agent) and you are attached to that same document and undo stack. sharedWithApp:false means a headless copy the app does not see. Returns file, path and modified. |
| sequence_status | true | Shared document file, path, dirty/render/undo status and sharedWithApp. |
| sequence_effects | true | List up to 10000 effects with stable in-session IDs, target UUIDs and millisecond times. |
| effect_add | false | Add an undoable effect to a target UUID. Optional params (name to value, as effect_parameters reports them) and layerId are applied in the same undo step. |
| effects_batch | false | Apply many effect edits as one undo step with one render pass. Every op is validated before anything changes; an invalid op fails the whole batch and names its index. Ops: {op:"add",typeId,targetId,startMs,durationMs,params?,layerId?}, {op:"delete",effectId}, {op:"timing",effectId,startMs,durationMs}, {op:"params",effectId,params}. Give ops inline, or opsFile: a .json path relative to the profile folder holding the same array (or {ops:[...]}), up to 100000 ops and 64 MiB, for shows too large for one request. Deletes need confirm:true. Returns effectIds of the adds in order. |
| sequence_clear | false | Delete every effect in the sequence as one undo step; marks, layers and length stay. Requires confirmation. |
| sequence_close | false | Close a sequence this Studio session created or opened, freeing it. If the app has it open in a window it stays open there (openInApp:true). Discarding unsaved changes requires confirm:true; save first to keep them. |
| effect_delete | false | Delete an effect; requires confirmation. |
| effect_timing | false | Move/resize an effect in milliseconds, in one undo step. |
| sequence_marks | true | List mark collections and timestamps. |
| marks_create | false | Create undoable timestamp marks, optionally authored phrase/word/phoneme text. No automatic speech recognition. |
| sequence_music_marks | false | Analyze profile media and append beat/bar collections to the sequence in one undo step. |
| sequence_undo | false | Undo one shared sequence edit. |
| sequence_redo | false | Redo one shared sequence edit. |
| sequence_render | false | Render dirty effects and wait for the real sequence schedule; no playback/output. |
| sequence_save | false | Save/overwrite a .tim filename under profile Sequence. Requires confirmation. |
| sequence_export_fseq | false | Render and export uncompressed FSEQ v2 at 50 ms under profile Export. Requires confirmation; no network transfer. |
| output_start | false | Start one E1.31/DDP controller only after execution is opened locally and the local Allow agents to control live output setting is enabled. |
| output_stop | false | Stop one controller. Requires explicit confirmation. Can stop even if agent output permission has been revoked. |
| output_test_pattern | false | Render a temporary Pulse on one target for 25..5000ms. Requires local agent-output permission, already-open execution and explicit confirmation. |
