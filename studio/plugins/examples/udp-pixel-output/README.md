# UDP Pixel Output (example)

A controller type in plain Node (20+) with `vixen-plugin.mjs` (a copy of `studio/plugins/sdk/typescript/vixen-plugin.mjs`,
types in `vixen-plugin.d.ts`): each frame goes out as raw RGB bytes in one UDP datagram. Try it:

    vixen-studio-mcp plugin validate . --live    # sends to 127.0.0.1:7777
    vixen-studio-mcp plugin run . --seconds 3

The `discover` tool (and Tools ▸ Plugins ▸ UDP Pixel Output ▸ Discover UDP Receivers) lists receivers that answer `VIXEN-DISCOVER`.
