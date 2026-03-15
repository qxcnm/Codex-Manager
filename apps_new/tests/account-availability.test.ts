import test from "node:test";
import assert from "node:assert/strict";
import { calcAvailability } from "../src/lib/utils/usage";

test("calcAvailability marks inactive account as expired", () => {
  const result = calcAvailability(
    {
      availabilityStatus: "unknown",
      usedPercent: 20,
      windowMinutes: 300,
      secondaryUsedPercent: 40,
      secondaryWindowMinutes: 10_080,
    },
    { status: "inactive" },
  );

  assert.equal(result.text, "已失效");
  assert.equal(result.level, "bad");
});

test("calcAvailability keeps exhausted inactive account as unavailable", () => {
  const result = calcAvailability(
    {
      availabilityStatus: "unavailable",
      usedPercent: 100,
      windowMinutes: 300,
      secondaryUsedPercent: 72,
      secondaryWindowMinutes: 10_080,
    },
    { status: "inactive" },
  );

  assert.equal(result.text, "不可用");
  assert.equal(result.level, "bad");
});

test("calcAvailability marks exhausted account as unavailable", () => {
  const result = calcAvailability(
    {
      availabilityStatus: "unavailable",
      usedPercent: 100,
      windowMinutes: 300,
      secondaryUsedPercent: 80,
      secondaryWindowMinutes: 10_080,
    },
    { status: "active" },
  );

  assert.equal(result.text, "不可用");
  assert.equal(result.level, "bad");
});
