import test from "node:test";
import assert from "node:assert/strict";

import { createSettingsController } from "../controller.js";

function createNormalizeAddr() {
  return (value) => {
    const raw = String(value || "").trim();
    if (!raw) {
      return "localhost:48760";
    }
    if (raw.includes(":")) {
      return raw;
    }
    return `localhost:${raw}`;
  };
}

function createController(overrides = {}) {
  return createSettingsController({
    dom: {},
    state: {},
    appSettingsGet: async () => ({}),
    appSettingsSet: async (patch = {}) => patch,
    showToast: () => {},
    normalizeErrorMessage: (err) => String(err?.message || err || ""),
    isTauriRuntime: () => false,
    normalizeAddr: createNormalizeAddr(),
    ...overrides,
  });
}

test("createSettingsController normalizes loaded snapshot and updates state.serviceAddr", async () => {
  const state = {};
  const controller = createController({
    state,
    appSettingsGet: async () => ({
      updateAutoCheck: 0,
      serviceAddr: "5050",
      serviceListenMode: "0.0.0.0",
      routeStrategy: "balanced",
      cpaNoCookieHeaderModeEnabled: "1",
      upstreamProxyUrl: " http://127.0.0.1:7890 ",
      sseKeepaliveIntervalMs: "16000",
      upstreamStreamTimeoutMs: "0",
      backgroundTasks: {
        usagePollIntervalSecs: "30",
      },
      envOverrides: {
        CODEXMANAGER_PROXY_URL: "http://127.0.0.1:7890",
      },
      envOverrideCatalog: [
        {
          key: "CODEXMANAGER_PROXY_URL",
          label: "上游代理",
          defaultValue: "",
          scope: "service",
          applyMode: "hot",
        },
      ],
      webAccessPasswordConfigured: 1,
    }),
  });

  const settings = await controller.loadAppSettings();

  assert.equal(settings.updateAutoCheck, false);
  assert.equal(settings.serviceAddr, "localhost:5050");
  assert.equal(settings.serviceListenMode, "all_interfaces");
  assert.equal(settings.routeStrategy, "balanced");
  assert.equal(settings.cpaNoCookieHeaderModeEnabled, true);
  assert.equal(settings.upstreamProxyUrl, "http://127.0.0.1:7890");
  assert.equal(settings.sseKeepaliveIntervalMs, 16000);
  assert.equal(settings.upstreamStreamTimeoutMs, 0);
  assert.equal(settings.backgroundTasks.usagePollIntervalSecs, 30);
  assert.equal(settings.webAccessPasswordConfigured, true);
  assert.equal(state.serviceAddr, "localhost:5050");
});

test("persistServiceAddrInput normalizes input and writes patch through settings API", async () => {
  const state = {};
  const dom = {
    serviceAddrInput: {
      value: " 6060 ",
    },
  };
  const patches = [];
  const controller = createController({
    dom,
    state,
    appSettingsSet: async (patch = {}) => {
      patches.push(patch);
      return patch;
    },
  });

  const ok = await controller.persistServiceAddrInput();

  assert.equal(ok, true);
  assert.equal(dom.serviceAddrInput.value, "localhost:6060");
  assert.equal(state.serviceAddr, "localhost:6060");
  assert.deepEqual(patches, [
    {
      serviceAddr: "localhost:6060",
    },
  ]);
});

test("createSettingsController exposes service listen mode actions used by main.js", () => {
  const controller = createController();

  assert.equal(typeof controller.applyServiceListenModeToService, "function");
  assert.equal(typeof controller.syncServiceListenModeOnStartup, "function");
  assert.equal(typeof controller.normalizeUpstreamProxyUrl, "function");
  assert.equal(typeof controller.initGatewayTransportSetting, "function");
  assert.equal(typeof controller.readGatewayTransportForm, "function");
});
