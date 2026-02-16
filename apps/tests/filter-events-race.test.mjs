import assert from "node:assert/strict";
import test from "node:test";

import { bindFilterEvents } from "../src/views/event-bindings/filter-events.js";

class FakeElement {
  constructor() {
    this.handlers = new Map();
  }

  addEventListener(type, handler) {
    this.handlers.set(type, handler);
  }

  dispatch(type, event) {
    const handler = this.handlers.get(type);
    if (!handler) return;
    return handler(event);
  }
}

function wait(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function buildDom(overrides = {}) {
  return {
    refreshRequestLogs: null,
    clearRequestLogs: null,
    requestLogSearch: null,
    filterLogAll: null,
    filterLog2xx: null,
    filterLog4xx: null,
    filterLog5xx: null,
    accountSearch: null,
    accountGroupFilter: null,
    filterAll: null,
    filterActive: null,
    filterLow: null,
    ...overrides,
  };
}

test("request log search only renders latest successful refresh", async () => {
  const requestLogSearch = new FakeElement();
  const state = {
    requestLogQuery: "",
    requestLogStatusFilter: "all",
    accountSearch: "",
    accountFilter: "all",
    accountGroupFilter: "all",
  };
  const calls = [];
  let renderCount = 0;

  bindFilterEvents({
    dom: buildDom({ requestLogSearch }),
    state,
    handleClearRequestLogs: () => {},
    refreshRequestLogs: async (query) => {
      calls.push(query);
      if (query === "old") {
        await wait(360);
        return false;
      }
      await wait(10);
      return true;
    },
    renderRequestLogs: () => {
      renderCount += 1;
    },
    renderAccountsView: () => {},
    updateRequestLogFilterButtons: () => {},
  });

  requestLogSearch.dispatch("input", { target: { value: "old" } });
  await wait(240);
  requestLogSearch.dispatch("input", { target: { value: "new" } });
  await wait(260);
  await wait(220);

  assert.deepEqual(calls, ["old", "new"]);
  assert.equal(renderCount, 1);
  assert.equal(state.requestLogQuery, "new");
});
