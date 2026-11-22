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
| `"type": "array"` of flat objects | A table: one row per item, with Add Row and remove. |
| `"type": "array"` of strings or numbers | A list. |
| anything else | JSON text. |

`title` labels a field (default: the property name), `description` explains it under the field, and `x-order` (an
array of property names on the object) sets the order; unlisted properties follow by name.

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

## Output naming

`outputNaming` (`"Pixel {n}"`) and `outputNames` answers from `controller.create` / `controller.setOutputCount` are
recorded but not applied to output names in this version.

## Lifecycle and timing

For each controller of your type in the show (see [PROTOCOL.md](PROTOCOL.md#lifecycle) for the full order):

1. Live Output turns on: `controller.create {instanceId, typeId, config, outputCount}` → `controller.start` (you have
   15 s to connect) → frames.
2. Edits while running: `controller.configure`, `controller.setOutputCount`.
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

**One RGB device per three outputs.** Set `defaultOutputs` to 3 per device (`maxOutputs` to 3 × the most devices you
support), list the devices in a table setting, and send device *k* outputs `3k, 3k+1, 3k+2`:

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

In `configure`, refuse a device list whose length × 3 is more than `outputCount`, naming the field (`/devices`).

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
