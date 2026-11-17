// Calls the example server with the generated JavaScript client. The same code runs in a
// browser page (as a module) once the server also serves the page.
//
//   gravenc gen schemas/shop.graven -o web/gen --lang js --side client
//   node web/client.mjs
import { RpcFault } from "./gen/graven.js";
import { ShopClient } from "./gen/shop.js";

const shop = new ShopClient(async (call, timeoutMs) => {
  const res = await fetch("http://127.0.0.1:3001/shop", {
    method: "POST",
    headers: { "content-type": "application/octet-stream" },
    body: call,
    signal: timeoutMs !== undefined ? AbortSignal.timeout(timeoutMs) : undefined,
  });
  return res.body; // read as it arrives: stream items come one by one
});

const r = await shop.get((w) => w.sku("cafe-500g"));
if (r.ok) console.log(`get: ${r.value.name()}, ${r.value.stock()} in stock`);

const missing = await shop.get((w) => w.sku("chá"));
if (!missing.ok && missing.error.variant === "NotFound") console.log(`get: no product ${missing.error.value.sku()}`);

for await (const p of await shop.list((w) => w.prefix("cafe"))) console.log(`list: ${p.sku()} ${p.stock()}`);

try {
  await shop.adjust((w) => w.sku("cafe-1kg").delta(5000));
} catch (e) {
  // A broken rule comes back as codes; the page turns them into text in its own language.
  if (e instanceof RpcFault && e.broken) {
    const b = e.broken;
    console.log(`adjust: ${b.field} breaks ${b.rule} (min ${b.min}, max ${b.max})`);
  } else throw e;
}
