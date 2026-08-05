import { test } from "node:test";
import assert from "node:assert/strict";
import { computeCountdownRemaining } from "./countdown.ts";

const now = Date.now();

test("no elapsed time → full chain value", () => {
  assert.equal(computeCountdownRemaining(300, now, now), 300);
});

test("degrades to pure wall-clock anchoring when ledger time is unavailable", () => {
  assert.equal(computeCountdownRemaining(300, now, now), 300);
  assert.equal(computeCountdownRemaining(300, now, now + 30_000), 270);
});

test("client clock skew is absorbed by anchoring to ledger close time", () => {
  // Client clock is 60s ahead of the ledger's close time. The countdown must
  // still show the chain-accurate remaining value (i.e. it hits 0 exactly when
  // the chain allows a draw).
  const ledgerClose = now;
  const fastNow = now + 60_000;
  assert.equal(computeCountdownRemaining(60, ledgerClose, fastNow), 0);
  assert.equal(computeCountdownRemaining(300, ledgerClose, fastNow), 240);
});

test("clamps to 0 below zero", () => {
  assert.equal(computeCountdownRemaining(10, now, now + 30_000), 0);
});

test("clamps to chainSeconds when elapsed is negative (slow client clock)", () => {
  const slowNow = now - 30_000;
  assert.equal(computeCountdownRemaining(300, now, slowNow), 300);
});

test("already-eligible draws read 0", () => {
  assert.equal(computeCountdownRemaining(0, now, now), 0);
  assert.equal(computeCountdownRemaining(-5, now, now), 0);
});
