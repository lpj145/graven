// Plain JavaScript, run by Node with no compilation: the generated `.js` files must work in a
// project without TypeScript. Importing a file that contains any type syntax fails here.

import { describe, it } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { Builder, FrameKind, encodeFrame, replyHeader } from "./gen/graven.js";
import { Root, RootW } from "./gen/conformance.js";
import { InventoryClient, ItemW } from "./gen/inventory.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const GEN = path.join(HERE, "gen");
const VECTORS = path.resolve(HERE, "../../testvectors/v1");

const vector = (name) => new Uint8Array(fs.readFileSync(path.join(VECTORS, `${name}.bin`)));

describe("generated JavaScript without TypeScript", () => {
  it("every generated module loads as plain JavaScript", async () => {
    const files = fs.readdirSync(GEN).filter((f) => f.endsWith(".js"));
    assert.ok(files.length > 10);
    for (const f of files) {
      await import(`./gen/${f}`);
    }
  });

  it("reads and writes the conformance vectors", () => {
    const all = Root.verified(vector("all_fields"));
    assert.equal(all.a_u8(), 200);
    assert.equal(all.a_i64(), -9_000_000_000_000_000_000n);
    assert.equal(all.name(), "grvn ✓ ação");

    const b = new Builder();
    RootW.root(b).a_u8(42).flag2(true);
    assert.deepEqual(b.finish(), vector("minimal"));
  });

  it("calls a service through the generated client", async () => {
    const client = new InventoryClient(async (_call, timeoutMs) => {
      assert.equal(timeoutMs, 2500);
      const b = new Builder();
      ItemW.root(b).sku("espresso").stock(7);
      return encodeFrame(replyHeader(FrameKind.Ok), b.finish());
    });
    const item = await client.get_item((w) => w.sku("espresso"));
    assert.equal(item.sku(), "espresso");
    assert.equal(item.stock(), 7);
  });
});
