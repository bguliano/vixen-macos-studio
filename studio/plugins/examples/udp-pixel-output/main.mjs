#!/usr/bin/env node
// UDP Pixel Output (example): sends each frame as raw RGB bytes, one UDP datagram per frame, to a host and port.
// Node 20 or later, no dependencies; vixen-plugin.mjs (a copy of studio/plugins/sdk/typescript) speaks the protocol.
//
// The datagram is the controller's outputs in order, three per pixel, reordered to the receiver's colour order. Frames
// are sent at most maxFps times a second, and an unchanged frame isn't sent again for a second, so an idle show doesn't
// flood the network. The discover tool broadcasts "VIXEN-DISCOVER\n" and lists the receivers that answer.
import { createSocket } from "node:dgram";
import { Plugin, Controller } from "./vixen-plugin.mjs";

const plugin = new Plugin();
const REPEAT_MS = 1000;
const STATS_MS = 1000;

class UdpPixels extends Controller {
  configure(config) {
    // configSchema has checked the host's form, the port range and the colour order.
    this.host = config.host;
    this.port = config.port;
    this.order = config.colorOrder;
    this.minGapMs = 1000 / config.maxFps;
  }

  start() {
    this.socket = createSocket("udp4");
    this.socket.on("error", (error) => this.status("error", error.message));
    this.lastSent = 0;
    this.lastBytes = null;
    this.pending = { packets: 0, bytes: 0, errors: 0 };
    this.statsTimer = setInterval(() => this.flushStats(), STATS_MS);
    this.status("running", `sending to ${this.host}:${this.port}`);
  }

  stop() {
    clearInterval(this.statsTimer);
    this.flushStats();
    this.socket?.close();
    this.socket = undefined;
  }

  frame(frame) {
    const now = Date.now();
    if (now - this.lastSent < this.minGapMs) return;
    const bytes = this.pack(frame.values.slice(0, this.outputCount));
    if (this.lastBytes && now - this.lastSent < REPEAT_MS && bytes.equals(this.lastBytes)) return;
    this.lastSent = now;
    this.lastBytes = bytes;
    this.socket.send(bytes, this.port, this.host, (error) => {
      if (error) {
        this.pending.errors += 1;
        this.status("unavailable", error.message);
      } else {
        this.pending.packets += 1;
        this.pending.bytes += bytes.length;
      }
    });
  }

  /** The outputs as bytes, each pixel's three reordered to the receiver's colour order; leftovers as they are. */
  pack(values) {
    const out = Buffer.from(values);
    const index = [...this.order].map((c) => "rgb".indexOf(c));
    for (let i = 0; i + 2 < values.length; i += 3) {
      for (let k = 0; k < 3; k += 1) out[i + k] = values[i + index[k]];
    }
    return out;
  }

  flushStats() {
    const { packets, bytes, errors } = this.pending ?? {};
    if (packets || errors) this.stats({ packetsSent: packets, bytesSent: bytes, errors });
    this.pending = { packets: 0, bytes: 0, errors: 0 };
  }
}

plugin.controller(UdpPixels);

plugin.tool("discover", ({ port = 7777, timeoutMs = 1000 }) => new Promise((resolve) => {
  const socket = createSocket("udp4");
  const found = [];
  socket.on("message", (message, from) => {
    if (found.length < 64) found.push(`${from.address}:${from.port} ${message.toString("utf8").split("\n")[0].slice(0, 64)}`);
  });
  socket.on("error", (error) => {
    socket.close();
    resolve({ content: [{ type: "text", text: `discovery failed: ${error.message}` }], isError: true });
  });
  socket.bind(0, () => {
    socket.setBroadcast(true);
    socket.send("VIXEN-DISCOVER\n", port, "255.255.255.255");
    setTimeout(() => {
      socket.close();
      resolve(found.length ? found.join("\n") : `No receivers answered on port ${port} within ${timeoutMs} ms.`);
    }, timeoutMs);
  });
}));

plugin.run();
