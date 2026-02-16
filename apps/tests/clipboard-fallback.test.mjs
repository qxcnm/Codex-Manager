import assert from "node:assert/strict";
import test from "node:test";

import { copyText } from "../src/utils/clipboard.js";

test("copyText uses navigator clipboard when available", async () => {
  let captured = "";
  const originalNavigator = globalThis.navigator;
  const originalDocument = globalThis.document;

  Object.defineProperty(globalThis, "navigator", {
    configurable: true,
    writable: true,
    value: {
      clipboard: {
        writeText: async (text) => {
          captured = text;
        },
      },
    },
  });
  Object.defineProperty(globalThis, "document", {
    configurable: true,
    writable: true,
    value: undefined,
  });

  const ok = await copyText("hello");
  assert.equal(ok, true);
  assert.equal(captured, "hello");

  Object.defineProperty(globalThis, "navigator", {
    configurable: true,
    writable: true,
    value: originalNavigator,
  });
  Object.defineProperty(globalThis, "document", {
    configurable: true,
    writable: true,
    value: originalDocument,
  });
});

test("copyText falls back to execCommand when clipboard api fails", async () => {
  const originalNavigator = globalThis.navigator;
  const originalDocument = globalThis.document;
  const appendCalls = [];
  const removeCalls = [];

  Object.defineProperty(globalThis, "navigator", {
    configurable: true,
    writable: true,
    value: {
      clipboard: {
        writeText: async () => {
          throw new Error("permission denied");
        },
      },
    },
  });

  const input = {
    value: "",
    style: {},
    setAttribute: () => {},
    select: () => {},
    setSelectionRange: () => {},
  };
  Object.defineProperty(globalThis, "document", {
    configurable: true,
    writable: true,
    value: {
      body: {
        appendChild: (node) => appendCalls.push(node),
        removeChild: (node) => removeCalls.push(node),
      },
      createElement: () => input,
      execCommand: (command) => command === "copy",
    },
  });

  const ok = await copyText("fallback");
  assert.equal(ok, true);
  assert.equal(input.value, "fallback");
  assert.equal(appendCalls.length, 1);
  assert.equal(removeCalls.length, 1);

  Object.defineProperty(globalThis, "navigator", {
    configurable: true,
    writable: true,
    value: originalNavigator,
  });
  Object.defineProperty(globalThis, "document", {
    configurable: true,
    writable: true,
    value: originalDocument,
  });
});
