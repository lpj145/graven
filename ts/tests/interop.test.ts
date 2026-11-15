import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { FaultCode, RpcFault, type Sender } from "./gen/graven.js";
import { InventoryClient } from "./gen/inventory.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const DEFAULT_BIN = path.resolve(HERE, "../../../target/debug/inventory_stdio");

function u32le(n: number): Uint8Array {
  const b = new Uint8Array(4);
  new DataView(b.buffer).setUint32(0, n, true);
  return b;
}

class StdioTransport {
  private readonly proc;
  private readonly iter: AsyncIterator<Uint8Array>;
  private stash = new Uint8Array(0);
  private queue: Promise<void> = Promise.resolve();
  private stderrText = "";

  constructor(binPath: string) {
    const proc = spawn(binPath, [], { stdio: ["pipe", "pipe", "pipe"] });
    this.proc = proc;
    this.iter = proc.stdout[Symbol.asyncIterator]();
    void (async () => {
      const dec = new TextDecoder();
      for await (const chunk of proc.stderr) {
        this.stderrText += dec.decode(chunk, { stream: true });
      }
    })();
  }

  private async readExact(n: number): Promise<Uint8Array> {
    while (this.stash.length < n) {
      const step = await this.iter.next();
      if (step.done) {
        throw new Error(`inventory_stdio closed early (needed ${n} bytes, had ${this.stash.length}). stderr: ${this.stderrText}`);
      }
      const chunk = step.value;
      const merged = new Uint8Array(this.stash.length + chunk.length);
      merged.set(this.stash, 0);
      merged.set(chunk, this.stash.length);
      this.stash = merged;
    }
    const out = this.stash.slice(0, n);
    this.stash = this.stash.slice(n);
    return out;
  }

  sender(chunkSize = 11): Sender {
    return (call: Uint8Array) => {
      const run = this.queue.then(async () => {
        const hdr = u32le(call.length);
        const packet = new Uint8Array(4 + call.length);
        packet.set(hdr, 0);
        packet.set(call, 4);
        await new Promise<void>((resolve, reject) => {
          this.proc.stdin.write(packet, (err) => (err ? reject(err) : resolve()));
        });
        const rlenBytes = await this.readExact(4);
        const rlen = new DataView(rlenBytes.buffer).getUint32(0, true);
        const reply = rlen === 0 ? new Uint8Array(0) : await this.readExact(rlen);
        if (reply.length === 0) return reply;
        // Return an async chunk stream so TS streaming decoder is exercised across frame boundaries.
        async function* splitChunks(): AsyncIterable<Uint8Array> {
          for (let i = 0; i < reply.length; i += chunkSize) {
            yield reply.subarray(i, Math.min(i + chunkSize, reply.length));
          }
        }
        return splitChunks();
      });
      this.queue = run.then(
        () => undefined,
        () => undefined,
      );
      return run;
    };
  }

  async close(): Promise<void> {
    this.proc.stdin.end();
    await new Promise<void>((resolve) => {
      this.proc.on("close", () => resolve());
    });
  }
}

describe("Rust <-> TypeScript RPC interop (Inventory)", () => {
  it("exercises unary, Result<T, U>, stream, Result<stream, E>, one-way, union, and validation Fault", async () => {
    const bin = process.env.INVENTORY_STDIO_BIN ?? DEFAULT_BIN;
    const tx = new StdioTransport(bin);
    const client = new InventoryClient(tx.sender(9));

    try {
      // 1. Unary -> Table
      const item = await client.get_item((w) => w.sku("espresso-blend"));
      assert.equal(item.sku(), "espresso-blend");
      assert.equal(item.stock(), 42);

      // 2. Unary -> Result<Receipt, ReserveErr> (Ok, Variant 1, Variant 2)
      const resOk = await client.reserve((w) => w.sku("espresso-blend").qty(3));
      assert.equal(resOk.ok, true);
      if (resOk.ok) {
        assert.equal(resOk.value.id(), 9001n);
      }

      const resOos = await client.reserve((w) => w.sku("espresso-blend").qty(50));
      assert.equal(resOos.ok, false);
      if (!resOos.ok) {
        assert.equal(resOos.error.variant, "OutOfStock");
        if (resOos.error.variant === "OutOfStock") {
          assert.equal(resOos.error.value.available(), 10);
        }
      }

      const resUnknown = await client.reserve((w) => w.sku("unknown").qty(1));
      assert.equal(resUnknown.ok, false);
      if (!resUnknown.ok) {
        assert.equal(resUnknown.error.variant, "InvalidSku");
        if (resUnknown.error.variant === "InvalidSku") {
          assert.equal(resUnknown.error.value.reason(), "unknown sku");
        }
      }

      // 3. Stream -> Item* + End
      const evStream = await client.watch((w) => w.sku("espresso-blend"));
      const events: Array<{ seq: bigint; stock: number }> = [];
      for await (const ev of evStream) {
        events.push({ seq: ev.seq(), stock: ev.stock() });
      }
      assert.deepEqual(events, [
        { seq: 1n, stock: 10 },
        { seq: 2n, stock: 8 },
      ]);

      // 3b. Stream -> Item + Fault mid-stream
      const badStream = await client.watch((w) => w.sku("fault-mid-stream"));
      const partial: bigint[] = [];
      await assert.rejects(
        (async () => {
          for await (const ev of badStream) {
            partial.push(ev.seq());
          }
        })(),
        (err: unknown) =>
          err instanceof RpcFault &&
          err.code === FaultCode.Unavailable &&
          err.message === "replica lost",
      );
      assert.deepEqual(partial, [1n]);

      // 4. Result<stream BatchItem, BatchErr> (Ok + Err)
      const batchOk = await client.batch((w) => w.count(4));
      assert.equal(batchOk.ok, true);
      if (batchOk.ok) {
        const indices: number[] = [];
        for await (const bi of batchOk.value) {
          indices.push(bi.index());
        }
        assert.deepEqual(indices, [0, 1, 2, 3]);
      }

      const batchErr = await client.batch((w) => w.count(99));
      assert.equal(batchErr.ok, false);
      if (!batchErr.ok) {
        assert.equal(batchErr.error.code(), 503);
      }

      // 5. One-way
      await client.audit((w) => w.actor("ts-client").note("interop check"));

      // 6. Union input + Result<Union, Table>
      const addRes = await client.dispatch((u) => u.Add().sku("decaf").delta(25));
      assert.equal(addRes.ok, true);
      if (addRes.ok) {
        assert.equal(addRes.value.variant, "Added");
        if (addRes.value.variant === "Added") {
          assert.equal(addRes.value.value.new_stock(), 125);
        }
      }

      const clearRes = await client.dispatch((u) => u.Clear().sku("decaf"));
      assert.equal(clearRes.ok, true);
      if (clearRes.ok) {
        assert.equal(clearRes.value.variant, "Cleared");
        if (clearRes.value.variant === "Cleared") {
          assert.equal(clearRes.value.value.removed(), 100);
        }
      }

      // 7. Server-side validation rejection produces Fault(Invalid) with BrokenRule
      await assert.rejects(
        client.reserve((w) => w.sku("espresso").qty(5000)),
        (err: unknown) =>
          err instanceof RpcFault &&
          err.code === FaultCode.Invalid &&
          err.broken?.tableName === "ReserveReq" &&
          err.broken?.field === "qty" &&
          err.broken?.rule === "range",
      );
    } finally {
      await tx.close();
    }
  });
});
