import { test } from "node:test";
import assert from "node:assert/strict";
import { computeWinProbability, computePoolShare } from "./probability.ts";

test("computeWinProbability: balance/total ratio as a percentage", () => {
  assert.equal(computeWinProbability(100n, 1000n), 10);
});

test("computeWinProbability: zero balance yields 0%", () => {
  assert.equal(computeWinProbability(0n, 1000n), 0);
});

test("computeWinProbability: empty pool yields 0% (guards division by zero)", () => {
  assert.equal(computeWinProbability(100n, 0n), 0);
  assert.equal(computeWinProbability(0n, 0n), 0);
});

test("computeWinProbability: never returns NaN/Infinity", () => {
  const v = computeWinProbability(1n, 3n);
  assert.ok(Number.isFinite(v));
  assert.ok(v > 33.3 && v < 33.4);
});

test("computePoolShare: fraction in [0, 1]", () => {
  assert.equal(computePoolShare(250n, 1000n), 0.25);
  assert.equal(computePoolShare(1000n, 1000n), 1);
});

test("computePoolShare: empty pool yields 0", () => {
  assert.equal(computePoolShare(100n, 0n), 0);
});
