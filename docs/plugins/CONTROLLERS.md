# Controller types

A controller type is the most common thing a plugin adds: a new kind of output, like E1.31 or DDP but for a device
Studio doesn't know (a Wi-Fi lamp, a cloud-controlled light, an MQTT bridge, a custom serial board). Once your plugin
declares one, it is in Add Controller under "Plugins", in `controller_types` for agents, and works with patching,
test patterns, the Controllers section's health and `controller_overview` like a built-in type.

## Outputs, frames and the engine

A Vixen controller has **outputs** (channels), numbered from 1. Elements are patched to outputs; at every frame (20–40
times a second) the engine works out each output's value and hands the controller the whole set. Your plugin receives
that set as one `controller.frame` per instance.

- With the usual `u8` encoding each output is one level, 0–255. An RGB pixel patched through Vixen's colour handling
  uses three consecutive outputs (R, G, B, in the element's channel order).
- Frames flow only while the controller is started: Live Output on and Studio not muted. Studio never sends
  `controller.start` or frames while muted (the headless MCP bridge, `VIXEN_MUTE`, Live Output off).
- **The engine never waits for your plugin.** Each instance has one frame slot: a newer frame replaces one you haven't
  read yet, counted as `framesDropped` in `plugin_info`. A slow plugin gets fewer frames, never stale ones. The SDKs
  keep the same promise inside your process: a slow `frame` handler skips to the newest frame instead of queueing.

## Frames

`frameEncodings` lists the encodings a type accepts, preferred first; Studio sends the first one.

| Encoding | `values` | Use it for |
|---|---|---|
| `u8` | base64 of one byte per output. An output without an 8-bit value sends 0. | Almost everything: dimmers, RGB pixels, DMX-like devices. Compact even for thousands of outputs. |
| `u16` | base64 of two bytes per output, little-endian; 8-bit values are scaled to the full range (×257). | Devices with 16-bit dimming. |
| `json` | an array, one item per output: `null` (no value), an integer, `{"r", "g", "b"}` (a colour) or a string. | Types using `oneChannelColor`, where one output carries a whole colour, or controllers that need Vixen's commands as they are. |

The SDKs decode `values` for you: `frame.values` is a list of numbers for `u8`/`u16` and the items as sent for `json`.

## Settings: `configSchema`

`configSchema` describes one controller's settings as a JSON Schema object. It does three jobs:

1. **The inspector form.** Studio builds the controller inspector from it: no window, no OK button; each finished edit
   is applied and is one step on the display's Undo.
2. **Checking.** Studio checks every configuration against it before your plugin sees it, and so do the SDKs. Problems
   show under their fields and the controller keeps its last good settings.
3. **Defaults.** A new controller starts with each property's `default` (nested objects filled in).

The supported subset (anything else is ignored by the checker, so your own `configure` stays the final word):
`type` (`object`, `string`, `number`, `integer`, `boolean`, `array`, `null`, or a list of them), `properties`,
`required`, `additionalProperties: false`, `enum`, `const`, `minimum`, `maximum`, `minLength`, `maxLength`, `items`,
`minItems`, `maxItems`, `default`, and `format` `ipv4` or `hostname`. Keep `additionalProperties: false`: it makes typos
errors and lets the harness test your refusals.

How each property becomes a field:

| Schema | Field |
|---|---|
| `"type": "string"` | A text field; `format: ipv4` and `hostname` add a matching placeholder and check. |
| `"type": "number"` / `"integer"` | A number field, checked against `minimum` / `maximum`. |
| `"type": "boolean"` | A switch. |
| `enum` | A choice; `x-enumNames` (an array the same length) gives each value its label. |
| `"type": "object"` with `properties` | A disclosure group with the nested fields. |
| `"type": "array"` of flat objects | A table: one row per item, with Add Row and remove. When its columns don't fit the inspector, each row shows as a card, labels above values. A new row starts from the item schema's defaults. |
| `"type": "array"` of strings or numbers | A list. |
| anything else | JSON text. |

`title` labels a field (default: the property name), `description` explains it under the field, and `x-order` (an
array of property names on the object, or on a table's `items`) sets the order; unlisted properties follow by name.
An integer `x-order` on a property also works (lowest first).

The inspector's ⋯ menu has **Copy Settings** and **Paste Settings**: the configuration as JSON, so you can put a
working example in your docs and users paste it in. A paste is checked like any edit and is one step on Undo.

Settings your schema can't express (a value that depends on another, a device that must be reachable) are checked in
your `configure`: refuse with `-32004` and `data.errors` (the SDKs: raise `ConfigError(message, field="/port")`). The
inspector shows your messages under the fields, and agents get them from `controller_configure`.

## Data policies

When several effects drive the same output at once, the engine combines them with the type's `dataPolicy`:

| `dataPolicy` | Combines | Use for |
|---|---|---|
| `highestWins8Bit` (default) | 8-bit levels, the highest wins. | Most lights. |
| `discardExcess8Bit` | 8-bit levels, the last one wins. | Devices where mixing makes no sense. |
| `average8Bit` | 8-bit levels, averaged. | Smooth blends on dimmers. |
| `oneChannelColor` | Whole colours on one output. Pair it with the `json` encoding. | Devices addressed by colour rather than by channel. |

## Output counts from the settings

When your settings decide how many outputs the controller needs (a table of lights, 3 outputs each), declare
`"outputCount": "fromConfig"` and let the plugin own the count (plugin API 1.1):

- After every settings edit (typing in the inspector, Paste Settings, `controller_configure`, a settings action) Studio
  asks `controller.outputsFor {typeId, config}` and resizes the controller to `outputCount` **in the same Undo step**
  as the edit. Patches on the outputs that remain stay where they are.
- New settings that would drop outputs that are patched aren't applied straight away: the inspector says which outputs
  would go and offers **Remove Outputs**; agents get a refusal naming them and pass `removePatchedOutputs: true`.
- `outputNames` in the answer, one per output, name the outputs (`Ceiling R`, `Ceiling G`, `Ceiling B`), so patching
  and the Controllers section read like the device.
- `lights` in the answer, `[{name, outputCount}]` in output order (1, 3 or 4 outputs each), lists the lights those
  outputs drive. Studio's **Add ▸ Single Bulbs…** then offers **Use the Controller's Lights**: one element per light,
  named after it, with the colour handling its outputs take, each patched to its own outputs, in one Undo step (agents:
  `controller_lights`, then `element_single_bulbs` with `fromController`). Without `lights`, outputs named per light
  (`Ceiling R`, `Ceiling G`, `Ceiling B`) are read the same way.
- The inspector shows the count read-only, saying your plugin sets it, instead of the Outputs… button; agents see
  `outputsFromConfig: true` in `controller_types` and `controller_network`, and `controller_configure` refuses `outputs`.
- Answer from `config` alone: no instance need exist (Live Output may be off), and Studio waits for you (5 s at most).
  When you can't answer, the settings still apply and the count stays as it was, with a warning.
- A new controller starts with `defaultOutputs` until its settings are first edited.

The SDKs answer `controller.outputsFor` for you: implement `outputs_for(cls, config)` as a class method (Python),
`static outputsFor(config)` (Node) or register `Plugin::outputs_for` (Rust), returning a count, a list of names, or
both; to list the lights too, return `vp.lights_outputs([(name, 3), …])` (Python), `lightsOutputs([[name, 3], …])`
(Node) or `Outputs::for_lights(…)` (Rust), which also name the outputs.

**Studio before plugin API 1.1** doesn't know these fields and refuses manifests with unknown ones. To keep one package
working there, put them at the root of `configSchema` as `x-outputCount` and `x-actions` (every Studio's checker ignores
`x-` keywords), and check `capabilities.controllerOutputs` from `initialize` (the SDKs: `plugin.host_sets_output_count`,
`plugin.hostSetsOutputCount`, `host.sets_output_count()`): without it, keep refusing settings that need more outputs
than `outputCount`, naming the count the user should set.

## Settings actions

`actions` adds buttons to the controller inspector (also in its ⋯ menu), like **Detect Lights**. Each names one of your
tools. Pressing it calls the tool with `{instanceId, typeId, config}`, the controller's current settings; answer the new
settings as `structuredContent` (or a JSON object as text; an SDK tool returning a dict does this):

- `{"config": {...}}`: the whole new configuration, or `{"mergePatch": {...}}`: an RFC 7396 merge patch against the
  current one (`null` removes a key; arrays are replaced whole).
- `"message"`: optional text the inspector shows under the button (`Found 5 lights; 2 already in the table`).
- Neither `config` nor `mergePatch`: nothing changes, and the message is shown.

Studio applies the answer like an edit: checked against `configSchema`, resized for `fromConfig` types, one step on
Undo. Agents run the same with `controller_action {id, action}`. The tool may take up to 30 s (a network scan); the
inspector shows progress meanwhile.

## Output naming

`outputNaming` (`"Pixel {n}"`) and the `outputNames` answers from `controller.create` / `controller.setOutputCount` are
recorded but not applied. Names from `controller.outputsFor` are applied (see above).

## Lifecycle and timing

For each controller of your type in the show (see [PROTOCOL.md](PROTOCOL.md#lifecycle) for the full order):

1. Live Output turns on: `controller.create {instanceId, typeId, config, outputCount}` → `controller.start` (you have
   15 s to connect) → frames.
2. Edits while running: `controller.configure`, `controller.setOutputCount`. A `fromConfig` type also gets
   `controller.outputsFor` before each settings edit, running or not.
3. Live Output turns off or the controller is removed: `controller.stop` → `controller.destroy`.

Report how it's going with `controller.status`: `running`, `unavailable` (can't reach the device; keep trying) or
`error`. The Controllers section and `controller_overview` show it as the controller's health. Report counters with
`controller.stats` (running totals, about once a second; the SDKs' `stats()` takes what you sent since the last
call and keeps the totals for you) and tell the user about something they must act on with
`controller.message`.

## Talking to slow devices

Frames arrive up to 40 times a second, but many devices (cloud APIs, some Wi-Fi lamps) accept far fewer commands.

- **Never block `frame` for long.** Store the newest values and hand the device I/O to a worker thread or task.
- **Send only on change**, and at most once per interval: the `rust-http-output` example posts at most once per
  `minIntervalMs`, and only when the colour changed.
- **Cap the rate** with a setting (`maxFps` in `udp-pixel-output`), so users can match the device.
- On a network error, report `unavailable` and retry later; don't fail the request or exit.

## Colour order

Vixen hands you outputs in the order the element's colour handling produced them, normally R, G, B. Many devices want
another order: offer a `colorOrder` setting (`rgb`, `grb`, `bgr`, …) and reorder when you send, as `udp-pixel-output`
does. The SDKs' `triplets(values, order)` helper groups outputs into `(r, g, b)` per light.

## Recipes

**One RGB device per three outputs.** Declare `"outputCount": "fromConfig"` (`maxOutputs` 3 × the most devices you
support), list the devices in a table setting, answer `controller.outputsFor` with 3 outputs per row named after the
device (and list the rows as `lights`, so Single Bulbs can make their elements), and send device *k* outputs
`3k, 3k+1, 3k+2`:

```json recipe configSchema
{
  "type": "object",
  "properties": {
    "devices": {
      "type": "array",
      "title": "Devices",
      "maxItems": 32,
      "items": {
        "type": "object",
        "properties": {
          "name": { "type": "string", "title": "Name" },
          "address": { "type": "string", "format": "ipv4", "title": "Address" }
        },
        "required": ["address"],
        "additionalProperties": false
      }
    }
  },
  "additionalProperties": false
}
```

```python
@plugin.controller()
class Lights(vp.Controller):
    @classmethod
    def outputs_for(cls, config):
        return vp.lights_outputs([(d.get("name") or d["address"], 3) for d in config["devices"]])
```

In `configure`, on a Studio without `controllerOutputs` (`not plugin.host_sets_output_count`), refuse a device list
whose length × 3 is more than `outputCount`, naming the field (`/devices`) and the count to set. With it, Studio has
already resized the controller.

**A pixel string over UDP.** `udp-pixel-output` sends each frame as raw bytes in one datagram, reordered per its
colour-order setting and capped at `maxFps`.

**A cloud API.** `rust-http-output` keeps the newest colour, and a worker posts it when it changes, at most once per
interval, counting failures as errors.

## What Studio stores

The controller lives in the show like any other: `SystemConfig.xml` has it and its outputs; `ModuleStore.xml` holds its
data under your `typeId` as `<PluginControllerData>` with your plugin's id, name, version and the settings as JSON.
That element is kept whole, so a save in Studio without your plugin, or by an older or newer Studio, writes it back
unchanged. Windows Vixen can't read the data type: it skips the entry while loading and leaves it out of its next
save, keeping the controller and its outputs.

Each save also writes `SystemData/StudioPlugins.json`, listing the plugins the show's controllers need
(`^major.minor` of the version each was set up with, `^0.minor.patch` for 0.x). Opened where the plugin is missing,
disabled, too old or crashed, the controller and its patches stay, its health says `Needs plugin Lamp Output
(com.example.lamp) ^1.2 — not installed` (or `— it is disabled`, `— version 1.1.0 is installed`), nothing is sent, and
the show window shows a banner. Installing or enabling the plugin brings it back without any edit.

## Live output and agents

A plugin controller is real output, gated like E1.31 and DDP: an agent can start it, or send it a test pattern with
`plugin_send_test_frame` / `output_test_pattern`, only when the user has turned on Settings ▸ Agents ▸ Allow agents to
control live output (off each time Studio starts) and Live Output is on. Turning the switch off stops what an agent
started.
