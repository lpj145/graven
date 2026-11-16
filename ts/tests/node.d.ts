declare module "node:test" {
  export function describe(name: string, fn: () => void | Promise<void>): void;
  export function it(name: string, fn: () => void | Promise<void>): void;
  export function test(name: string, fn: () => void | Promise<void>): void;
}

declare module "node:assert/strict" {
  export function equal<T>(actual: unknown, expected: T, message?: string): asserts actual is T;
  export function deepEqual<T>(actual: unknown, expected: T, message?: string): asserts actual is T;
  export function ok(value: unknown, message?: string): asserts value;
  export function throws(fn: () => unknown, error?: unknown, message?: string): void;
  export function rejects(fn: (() => Promise<unknown>) | Promise<unknown>, error?: unknown): Promise<void>;
  export function fail(message?: string): never;
}

declare module "node:fs" {
  export function readdirSync(path: string): string[];
  export function readFileSync(path: string, encoding: "utf8"): string;
  export function readFileSync(path: string): Uint8Array;
  export function writeFileSync(path: string, data: string | Uint8Array): void;
  export function mkdirSync(path: string, options?: { recursive?: boolean }): void;
  export function rmSync(path: string, options?: { recursive?: boolean; force?: boolean }): void;
  export function mkdtempSync(prefix: string): string;
  export function existsSync(path: string): boolean;
}

declare module "node:path" {
  export function join(...paths: string[]): string;
  export function resolve(...paths: string[]): string;
  export function dirname(path: string): string;
}

declare module "node:url" {
  export function fileURLToPath(url: string | URL): string;
}

declare module "node:child_process" {
  export interface ChildProcessWithoutNullStreams {
    readonly stdin: {
      write(chunk: Uint8Array, cb?: (err?: Error | null) => void): boolean;
      end(): void;
    };
    readonly stdout: AsyncIterable<Uint8Array>;
    readonly stderr: AsyncIterable<Uint8Array>;
    on(event: "close", cb: (code: number | null) => void): this;
    kill(): void;
  }
  export function spawn(
    command: string,
    args?: readonly string[],
    options?: { stdio?: readonly string[]; cwd?: string },
  ): ChildProcessWithoutNullStreams;
  export function spawnSync(
    command: string,
    options: { input: string; maxBuffer?: number },
  ): { readonly status: number | null; readonly stdout: Uint8Array; readonly stderr: Uint8Array };
}

declare module "node:util" {
  export function isDeepStrictEqual(a: unknown, b: unknown): boolean;
}

declare const process: {
  readonly env: Record<string, string | undefined>;
  readonly argv: readonly string[];
  cwd(): string;
  exit(code?: number): never;
};
