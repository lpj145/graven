import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { ValidationError } from "./gen/graven.js";
import {
  type Checks,
  Promo,
  PromoW,
  Reading,
  ReadingW,
  Status,
} from "./gen/validation.js";

const appChecks: Checks = {
  slug: (v) => v.length > 0 && /^[A-Za-z0-9-]+$/.test(v),
  valid_period: (p) => p.starts() < p.ends(),
};

describe("validation codegen (validation.ts)", () => {
  it("validate and toBytesValidated pass when valid", () => {
    const bytes = PromoW.toBytesValidated(
      (p) => p.price(150).starts(10n).ends(20n),
      appChecks,
    );
    const view = Promo.fromBytes(bytes);
    view.validate(appChecks);
    assert.deepEqual(view.validateAll(appChecks), []);
  });

  it("reports field, list index, known, finite, UTF-8 byte len, and table check", () => {
    // 1. toBytesValidated rejects invalid range
    assert.throws(
      () => PromoW.toBytesValidated((p) => p.price(0).starts(10n).ends(20n), appChecks),
      (err: unknown) =>
        err instanceof ValidationError &&
        err.failure.table === "Promo" &&
        err.failure.field === "price" &&
        err.failure.index === undefined &&
        err.failure.rule === "range",
    );

    // 2. Failure inside nested [Item] list records element index
    const bytes = PromoW.toBytes((p) => {
      p.price(100).starts(10n).ends(20n);
      const items = p.items(2);
      items.push().sku("valid-1").qty(5).status(Status.Active);
      items.push().sku("bad sku!").qty(5).status(Status.Draft);
    });
    const view = Promo.fromBytes(bytes);
    assert.throws(
      () => view.validate(appChecks),
      (err: unknown) =>
        err instanceof ValidationError &&
        err.failure.table === "Item" &&
        err.failure.field === "sku" &&
        err.failure.index === 1 &&
        err.failure.rule === "check(slug)",
    );

    // 3. `known` on [Status] records element index
    const badStatuses = PromoW.toBytes((p) => {
      p.price(100).starts(10n).ends(20n);
      p.statuses([Status.Active, 99]);
    });
    assert.throws(
      () => Promo.fromBytes(badStatuses).validate(appChecks),
      (err: unknown) =>
        err instanceof ValidationError &&
        err.failure.table === "Promo" &&
        err.failure.field === "statuses" &&
        err.failure.index === 1 &&
        err.failure.rule === "known",
    );

    // 4. Table-level #[check(valid_period)] omits `field`
    const badPeriod = PromoW.toBytes((p) => p.price(100).starts(30n).ends(20n));
    assert.throws(
      () => Promo.fromBytes(badPeriod).validate(appChecks),
      (err: unknown) =>
        err instanceof ValidationError &&
        err.failure.table === "Promo" &&
        err.failure.field === undefined &&
        err.failure.index === undefined &&
        err.failure.rule === "check(valid_period)",
    );

    // 5. #[len] on string measures UTF-8 bytes, not UTF-16 code units:
    //    9 4-byte emoji = 18 JS code units (<= 32), 36 UTF-8 bytes (> 32)!
    const emoji9 = "🦀".repeat(9);
    assert.equal(emoji9.length, 18);
    const badUtf8Len = PromoW.toBytes((p) => {
      p.price(100).starts(10n).ends(20n);
      p.items(1).push().sku(emoji9).qty(1).status(Status.Active);
    });
    const lenFailures = Promo.fromBytes(badUtf8Len).validateAll({
      slug: () => true,
      valid_period: () => true,
    });
    assert.deepEqual(lenFailures, [
      { table: "Item", field: "sku", index: 0, rule: "len" },
    ]);
  });

  it("validateAll collects every failure across finite, range, len, known, and checks", () => {
    const bytes = PromoW.toBytes((p) => {
      p.price(0)
        .discount(Number.POSITIVE_INFINITY)
        .tags(["a", "b", "c", "d", "e", "f"])
        .statuses([Status.Active, 77])
        .starts(50n)
        .ends(10n);
      const items = p.items(1);
      items.push().sku("bad!").qty(200).status(42);
    });
    const failures = Promo.fromBytes(bytes).validateAll(appChecks);
    assert.deepEqual(failures, [
      { table: "Promo", field: "price", rule: "range" },
      { table: "Promo", field: "discount", rule: "range" },
      { table: "Promo", field: "discount", rule: "finite" },
      { table: "Promo", field: "tags", rule: "len" },
      { table: "Promo", field: "statuses", index: 1, rule: "known" },
      { table: "Item", field: "sku", index: 0, rule: "check(slug)" },
      { table: "Item", field: "qty", index: 0, rule: "range" },
      { table: "Item", field: "status", index: 0, rule: "known" },
      { table: "Promo", rule: "check(valid_period)" },
    ]);

    // Reading also validates with appChecks (and schemas without custom checks default `checks = {}`).
    const goodReading = ReadingW.toBytesValidated((r) => r.celsius(-10), appChecks);
    Reading.fromBytes(goodReading).validate(appChecks);
  });
});
