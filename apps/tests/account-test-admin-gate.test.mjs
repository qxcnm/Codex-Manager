import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";

const appsRoot = path.resolve(import.meta.dirname, "..");
const pageSource = await fs.readFile(
  path.join(appsRoot, "src", "app", "accounts", "page.tsx"),
  "utf8",
);
const viewSource = await fs.readFile(
  path.join(appsRoot, "src", "app", "accounts", "accounts-page-view.tsx"),
  "utf8",
);
const modalSource = await fs.readFile(
  path.join(appsRoot, "src", "components", "modals", "account-test-modal.tsx"),
  "utf8",
);

test("account test UI is derived from the current admin session", () => {
  assert.match(pageSource, /useAppSession\(\)/);
  assert.match(
    pageSource,
    /resolveSessionRole\(session, isSessionLoading, isDesktopRuntime\)/,
  );
  assert.match(
    pageSource,
    /const canTestAccounts\s*=\s*isDesktopRuntime \|\|\s*\(!isSessionLoading && isAdminRole\(role\)\)/,
  );
  assert.match(
    pageSource,
    /const openAccountTest = \(account: Account\) => \{\s*if \(!canTestAccounts\) return;/,
  );
  assert.match(pageSource, /canTestAccounts=\{canTestAccounts\}/);
});

test("account test menu and modal are both hidden from non-admin views", () => {
  assert.match(
    viewSource,
    /\{props\.canTestAccounts \? \(\s*<DropdownMenuItem[\s\S]*?t\("测试账号"\)[\s\S]*?\) : null\}/,
  );
  assert.match(
    viewSource,
    /\{props\.canTestAccounts \? \(\s*<AccountTestModal[\s\S]*?\) : null\}/,
  );
});

test("account test IDs never fall back to predictable browser randomness", () => {
  assert.match(modalSource, /cryptoApi\.getRandomValues\(new Uint8Array\(16\)\)/);
  assert.doesNotMatch(modalSource, /Math\.random|Date\.now/);
});
