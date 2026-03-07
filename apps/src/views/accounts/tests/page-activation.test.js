import test from "node:test";
import assert from "node:assert/strict";

import { activateAccountsPage } from "../page-activation.js";

test("activateAccountsPage skips immediate render before first paged load", async () => {
  const renderedPages = [];
  const seenReloadOptions = [];

  const result = await activateAccountsPage({
    accountPageLoaded: false,
    renderCurrentPageView: (page) => {
      renderedPages.push(page);
    },
    reloadAccountsPage: async (options) => {
      seenReloadOptions.push(options);
      return true;
    },
  });

  assert.equal(result, true);
  assert.deepEqual(renderedPages, []);
  assert.deepEqual(seenReloadOptions, [{ silent: true, latestOnly: true }]);
});

test("activateAccountsPage keeps current page responsive when paged data already exists", async () => {
  const renderedPages = [];
  const seenReloadOptions = [];

  await activateAccountsPage({
    accountPageLoaded: true,
    renderCurrentPageView: (page) => {
      renderedPages.push(page);
    },
    reloadAccountsPage: async (options) => {
      seenReloadOptions.push(options);
      return true;
    },
    reloadOptions: { ensureConnection: false },
  });

  assert.deepEqual(renderedPages, ["accounts"]);
  assert.deepEqual(seenReloadOptions, [{
    silent: true,
    latestOnly: true,
    ensureConnection: false,
  }]);
});
