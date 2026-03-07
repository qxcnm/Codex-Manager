import test from "node:test";
import assert from "node:assert/strict";

import { state } from "../../state.js";
import { refreshAccountsPage, refreshRequestLogs } from "../data.js";

function deferred() {
  let resolve = null;
  let reject = null;
  const promise = new Promise((res, rej) => {
    resolve = res;
    reject = rej;
  });
  // 中文注释：避免某些取消路径下 deferred promise 未被 await 时触发 unhandledRejection。
  promise.catch(() => {});
  return { promise, resolve, reject };
}

test("refreshRequestLogs aborts stale request when query changes", async () => {
  const oldWindow = globalThis.window;
  const oldFetch = globalThis.fetch;
  const first = deferred();
  const second = deferred();
  const seenQueries = [];

  try {
    globalThis.window = {
      __TAURI__: {
        core: {
          invoke: async (method) => {
            if (method === "service_rpc_token") {
              return "test-token";
            }
            throw new Error(`unexpected invoke: ${method}`);
          },
        },
      },
    };
    globalThis.fetch = async (_url, options) => {
      const signal = options && options.signal;
      const query = JSON.parse(options.body).params.query;
      seenQueries.push(query);
      if (query === "old") {
        await first.promise;
        return {
          ok: true,
          json: async () => ({ result: { items: [{ id: "old" }] } }),
        };
      }
      await second.promise;
      return {
        ok: true,
        json: async () => ({ result: { items: [{ id: "new" }] } }),
      };
    };

    state.serviceAddr = "localhost:48760";
    state.requestLogList = [];

    const oldTask = refreshRequestLogs("old", { latestOnly: true });
    await Promise.resolve();
    const newTask = refreshRequestLogs("new", { latestOnly: true });

    first.reject(new DOMException("The operation was aborted.", "AbortError"));
    second.resolve();

    const oldApplied = await oldTask;
    const newApplied = await newTask;

    assert.equal(oldApplied, false);
    assert.equal(newApplied, true);
    assert.ok(seenQueries.includes("new"));
    assert.equal(state.requestLogList.length, 1);
    assert.equal(state.requestLogList[0].id, "new");
    assert.ok(state.requestLogList[0].__identity);
  } finally {
    globalThis.window = oldWindow;
    globalThis.fetch = oldFetch;
  }
});

test("refreshAccountsPage falls back to local mode when backend does not return pagination fields", async () => {
  const oldWindow = globalThis.window;

  try {
    globalThis.window = {
      __TAURI__: {
        core: {
          invoke: async (method, params) => {
            if (method === "service_account_list") {
              assert.equal(params.page, 1);
              assert.equal(params.pageSize, 5);
              return {
                result: {
                  items: [
                    { id: "acc-1", label: "账号1", groupName: "A组", sort: 1 },
                    { id: "acc-2", label: "账号2", groupName: "A组", sort: 2 },
                  ],
                },
              };
            }
            throw new Error(`unexpected invoke: ${method}`);
          },
        },
      },
    };

    state.accountList = [];
    state.accountPage = 1;
    state.accountPageSize = 5;
    state.accountSearch = "";
    state.accountFilter = "all";
    state.accountGroupFilter = "all";
    state.accountPageItems = [];
    state.accountPageTotal = 0;
    state.accountPageLoaded = false;

    const applied = await refreshAccountsPage({ latestOnly: true });

    assert.equal(applied, true);
    assert.equal(state.accountPageLoaded, false);
    assert.equal(state.accountPageTotal, 0);
    assert.equal(state.accountPageItems.length, 0);
    assert.equal(state.accountList.length, 2);
    assert.equal(state.accountList[0].id, "acc-1");
  } finally {
    globalThis.window = oldWindow;
  }
});

test("refreshAccountsPage stores remote page state when backend pagination is available", async () => {
  const oldWindow = globalThis.window;

  try {
    globalThis.window = {
      __TAURI__: {
        core: {
          invoke: async (method, params) => {
            if (method === "service_account_list") {
              assert.equal(params.page, 3);
              assert.equal(params.pageSize, 10);
              assert.equal(params.query, "team");
              assert.equal(params.filter, "active");
              assert.equal(params.groupFilter, "A组");
              return {
                result: {
                  items: [
                    { id: "acc-21", label: "账号21", groupName: "A组", sort: 21 },
                    { id: "acc-22", label: "账号22", groupName: "A组", sort: 22 },
                  ],
                  total: 28,
                  page: 3,
                  pageSize: 10,
                },
              };
            }
            throw new Error(`unexpected invoke: ${method}`);
          },
        },
      },
    };

    state.accountList = [{ id: "legacy-1", label: "旧账号" }];
    state.accountPage = 3;
    state.accountPageSize = 10;
    state.accountSearch = "team";
    state.accountFilter = "active";
    state.accountGroupFilter = "A组";
    state.accountPageItems = [];
    state.accountPageTotal = 0;
    state.accountPageLoaded = false;

    const applied = await refreshAccountsPage({ latestOnly: true });

    assert.equal(applied, true);
    assert.equal(state.accountPageLoaded, true);
    assert.equal(state.accountPageTotal, 28);
    assert.equal(state.accountPage, 3);
    assert.equal(state.accountPageSize, 10);
    assert.deepEqual(
      state.accountPageItems.map((item) => item.id),
      ["acc-21", "acc-22"],
    );
    assert.deepEqual(
      state.accountList.map((item) => item.id),
      ["legacy-1"],
    );
  } finally {
    globalThis.window = oldWindow;
  }
});
