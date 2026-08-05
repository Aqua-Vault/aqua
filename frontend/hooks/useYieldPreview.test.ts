import { test } from "node:test";
import assert from "node:assert/strict";
import { projectYield, apyPercent } from "../lib/yield.ts";

const SCALE = 10n ** 7n;
const YEAR = 31_536_000;

test("projectYield: one year at 10% on 100k => 10k yield", () => {
  const out = projectYield(100_000n * SCALE, 1_000, YEAR);
  assert.equal(out, 10_000n * SCALE);
});

test("projectYield: half a year at 10% => half the yield", () => {
  const out = projectYield(100_000n * SCALE, 1_000, YEAR / 2);
  assert.equal(out, 5_000n * SCALE);
});

test("projectYield: includes already-accrued yield", () => {
  const out = projectYield(100_000n * SCALE, 1_000, YEAR, 3_000n * SCALE);
  assert.equal(out, 13_000n * SCALE);
});

test("projectYield: zero rate / zero seconds / zero principal => 0", () => {
  assert.equal(projectYield(100_000n * SCALE, 0, YEAR), 0n);
  assert.equal(projectYield(100_000n * SCALE, 1_000, 0), 0n);
  assert.equal(projectYield(0n, 1_000, YEAR), 0n);
});

test("projectYield: stays in raw 7-decimal units (no float drift)", () => {
  // 333 USDC @ 10%/yr for one year = exactly 33.30 USDC.
  const out = projectYield(333n * SCALE, 1_000, YEAR);
  assert.equal(out, 333_000_000n);
});

test("apyPercent: bps -> percentage string", () => {
  assert.equal(apyPercent(1_000), "10.00");
  assert.equal(apyPercent(2_500), "25.00");
  assert.equal(apyPercent(0), "0.00");
});
