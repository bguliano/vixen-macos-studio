// Types for vixen-plugin.mjs, the Vixen Studio plugin SDK for Node (plugin API 1.0).

export const VERSION: string;
export const API_VERSION: string;
export const METHOD_NOT_FOUND: -32601;
export const INVALID_PARAMS: -32602;
export const INTERNAL_ERROR: -32603;
export const INVALID_CONFIG: -32004;

export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
export type Config = Record<string, Json>;
export interface FieldError {
  /** JSON Pointer of the setting, "/" for the whole configuration. */
  field: string;
  message: string;
}

export class ConfigError extends Error {
  constructor(errors: FieldError[] | string, field?: string);
  errors: FieldError[];
}
export class ToolError extends Error {}
export class RpcError extends Error {
  constructor(code: number, message: string, data?: Json);
  code: number;
  data?: Json;
}

/** A frame value with the json encoding: off, a level, a colour or a command string. */
export type FrameValue = null | number | { r: number; g: number; b: number } | string;

export interface Frame {
  /** 0-255 per output (u8), 0-65535 (u16), or FrameValue items (json). */
  values: number[] | FrameValue[];
  seq: number;
  timeMs: number;
  encoding: "u8" | "u16" | "json";
}

export interface ToolContent {
  content: Array<{ type: string; text?: string; [key: string]: unknown }>;
  isError?: boolean;
}

export interface InitializeParams {
  apiVersion: string;
  hostVersion: string;
  pluginDataDir: string;
  profile: { name: string; id: string } | null;
  muted: boolean;
  capabilities: Record<string, unknown>;
}

/** One page of display.snapshot (or every page, from Plugin.displaySnapshot). Outputs are one-based. */
export interface DisplaySnapshot {
  revision: number;
  roots: string[];
  controllers: Array<{ id: string; name: string; typeId: string; typeName: string; outputs: number; running: boolean; instanceId?: string }>;
  elements: Array<{ id: string; name: string; children: string[] }>;
  patches: Array<{ elementId: string; controllerId: string; firstOutput: number; lastOutput: number; from: string; to: string }>;
  elementCount: number;
  patchCount: number;
  more: boolean;
  elementOffset: number;
  patchOffset: number;
}

export type Awaitable<T> = T | Promise<T>;

export function decode(encoding: string, values: unknown): number[] | FrameValue[];
export function triplets(values: number[], order?: string): Array<[number, number, number]>;
export function validate(schema: object, value: unknown, at?: string): FieldError[];
export function fillDefaults(schema: object, config: Config): Config;
export function loadManifest(path?: string): Record<string, any>;

export class Controller {
  constructor(plugin: Plugin, instanceId: string, typeId: string, outputCount: number);
  readonly plugin: Plugin;
  readonly instanceId: string;
  readonly typeId: string;
  outputCount: number;
  config: Config;
  running: boolean;
  paused: boolean;
  /** The checked configuration, on create and every edit. Throw ConfigError to refuse; return a normalised config or nothing. */
  configure(config: Config): Awaitable<Config | void>;
  setOutputCount(count: number): Awaitable<void>;
  start(): Awaitable<void>;
  stop(): Awaitable<void>;
  frame(frame: Frame): Awaitable<void>;
  destroy(): Awaitable<void>;
  outputNames(): string[] | undefined;
  status(state: "running" | "unavailable" | "error", message?: string): void;
  stats(counters?: { packetsSent?: number; bytesSent?: number; errors?: number }): void;
  message(title: string, text: string, heading?: string): void;
}

export type ControllerClass = new (plugin: Plugin, instanceId: string, typeId: string, outputCount: number) => Controller;

export class Plugin {
  constructor(manifestPath?: string);
  readonly manifest: Record<string, any>;
  dataDir: string | undefined;
  muted: boolean;
  profile: { name: string; id: string } | null;
  hostVersion: string | null;
  controller<C extends ControllerClass>(cls: C, options?: { typeId?: string }): C;
  tool(name: string, fn: (args: Record<string, Json>) => Awaitable<string | ToolContent | Json>): void;
  event(name: string, fn: (data: Json) => Awaitable<void>): void;
  onInitialize(fn: (params: InitializeParams) => Awaitable<void>): void;
  onShutdown(fn: () => Awaitable<void>): void;
  send(message: Record<string, unknown>): void;
  notify(method: string, params: Record<string, unknown>): void;
  request(method: string, params: Record<string, unknown>, timeoutMs?: number): Promise<any>;
  log(level: "debug" | "info" | "warn" | "error", message: unknown): void;
  debug(message: unknown): void;
  info(message: unknown): void;
  warn(message: unknown): void;
  error(message: unknown): void;
  storageGet<T = Json>(key: string, fallback?: T): Promise<T>;
  storageSet(key: string, value: Json): Promise<void>;
  /** Every page of display.snapshot gathered into one object. Needs "display": "read". */
  displaySnapshot(): Promise<DisplaySnapshot>;
  toolsChanged(tools?: Array<Record<string, unknown>>): void;
  run(): void;
}
