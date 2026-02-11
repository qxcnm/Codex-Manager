import test from "node:test";
import assert from "node:assert/strict";
import { filterAccounts } from "../src/views/accounts.js";

test("filterAccounts matches search, status and group", () => {
  const accounts = [
    { id: "a", label: "alpha", groupName: "TEAM" },
    { id: "b", label: "bravo", groupName: "PERSONAL" },
  ];
  const usage = [{ accountId: "a", usedPercent: 90, secondaryUsedPercent: 10 }];
  const out = filterAccounts(accounts, usage, "alp", "low", "TEAM");
  assert.equal(out.length, 1);
  assert.equal(out[0].id, "a");
});
