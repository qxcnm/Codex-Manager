import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { cleanup, render, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AccountRowActionsMenu } from "../src/components/accounts/account-row-actions-menu";
import type { Account } from "../src/types";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost/accounts",
});

globalThis.window = dom.window as unknown as Window & typeof globalThis;
globalThis.document = dom.window.document;
globalThis.HTMLElement = dom.window.HTMLElement;
globalThis.Element = dom.window.Element;
globalThis.Node = dom.window.Node;
globalThis.DocumentFragment = dom.window.DocumentFragment;
globalThis.MutationObserver = dom.window.MutationObserver;
globalThis.PointerEvent =
  (dom.window.PointerEvent as typeof globalThis.PointerEvent | undefined) ??
  (dom.window.MouseEvent as unknown as typeof globalThis.PointerEvent);
globalThis.getComputedStyle = dom.window.getComputedStyle.bind(dom.window);
globalThis.requestAnimationFrame = (callback: FrameRequestCallback) =>
  setTimeout(() => callback(performance.now()), 0) as unknown as number;
globalThis.cancelAnimationFrame = (handle: number) => clearTimeout(handle);
Object.defineProperty(globalThis, "navigator", {
  configurable: true,
  value: dom.window.navigator,
});

function createAccount(overrides: Partial<Account> = {}): Account {
  return {
    id: overrides.id || "acc-1",
    name: overrides.name || "Demo Account",
    group: overrides.group || "默认",
    priority: overrides.priority ?? 0,
    label: overrides.label || overrides.name || "Demo Account",
    groupName: overrides.groupName || overrides.group || "默认",
    sort: overrides.sort ?? 0,
    status: overrides.status || "active",
    isAvailable: overrides.isAvailable ?? true,
    availabilityKind: overrides.availabilityKind || "available",
    isLowQuota: overrides.isLowQuota ?? false,
    lastRefreshAt: overrides.lastRefreshAt ?? null,
    availabilityText: overrides.availabilityText || "可用",
    availabilityLevel: overrides.availabilityLevel || "ok",
    primaryRemainPercent: overrides.primaryRemainPercent ?? null,
    secondaryRemainPercent: overrides.secondaryRemainPercent ?? null,
    usage: overrides.usage ?? null,
  };
}

test("AccountRowActionsMenu calls delete handler when delete item is clicked", async () => {
  const user = userEvent.setup({ document: dom.window.document });
  const calls: Account[] = [];

  const view = render(
    <AccountRowActionsMenu
      account={createAccount()}
      onOpenDetails={() => {}}
      onDelete={(account) => calls.push(account)}
    />,
  );

  await user.click(view.getByRole("button", { name: "删除账号" }));
  await view.findByText("确定删除账号 Demo Account 吗？");
  assert.equal(calls.length, 0);
  await user.click(view.getByRole("button", { name: "确认删除" }));

  await waitFor(() => {
    assert.equal(calls.length, 1);
    assert.equal(calls[0]?.id, "acc-1");
  });

  cleanup();
});
