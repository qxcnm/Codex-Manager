import test from "node:test";
import assert from "node:assert/strict";

import { buildUsageRows } from "./usage-table.js";

test("buildUsageRows maps account usage into row data", () => {
  const accountList = [
    { id: "acc-1", label: "main" },
    { id: "acc-2", label: "secondary" },
  ];
  const usageList = [
    {
      accountId: "acc-1",
      usedPercent: 10,
      secondaryUsedPercent: 20,
      resetsAt: 111,
      secondaryResetsAt: 222,
    },
  ];

  const rows = buildUsageRows(accountList, usageList);
  assert.equal(rows.length, 2);
  assert.equal(rows[0].accountLabel, "main");
  assert.equal(rows[0].accountSub, "acc-1");
  assert.equal(rows[0].primaryRemain, 90);
  assert.equal(rows[0].secondaryRemain, 80);
  assert.equal(rows[0].primaryResetsAt, 111);
  assert.equal(rows[0].secondaryResetsAt, 222);
  assert.equal(rows[1].accountSub, "acc-2");
  assert.equal(rows[1].primaryRemain, null);
  assert.equal(rows[1].secondaryRemain, null);
});

