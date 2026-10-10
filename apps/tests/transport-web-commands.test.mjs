import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";
import ts from "../node_modules/typescript/lib/typescript.js";

const appsRoot = path.resolve(import.meta.dirname, "..");
const sourcePath = path.join(appsRoot, "src", "lib", "api", "transport-web-commands.ts");
const modulePaths = [
  path.join(appsRoot, "src", "lib", "api", "transport-web-commands", "account.ts"),
  path.join(appsRoot, "src", "lib", "api", "transport-web-commands", "aggregate-api.ts"),
  path.join(appsRoot, "src", "lib", "api", "transport-web-commands", "apikey.ts"),
  path.join(appsRoot, "src", "lib", "api", "transport-web-commands", "browser-direct.ts"),
  path.join(appsRoot, "src", "lib", "api", "transport-web-commands", "codex-profile.ts"),
  path.join(appsRoot, "src", "lib", "api", "transport-web-commands", "codex-skills.ts"),
  path.join(appsRoot, "src", "lib", "api", "transport-web-commands", "gateway.ts"),
  path.join(appsRoot, "src", "lib", "api", "transport-web-commands", "login.ts"),
  path.join(appsRoot, "src", "lib", "api", "transport-web-commands", "misc.ts"),
  path.join(appsRoot, "src", "lib", "api", "transport-web-commands", "proxy-profiles.ts"),
  path.join(appsRoot, "src", "lib", "api", "transport-web-commands", "quota.ts"),
  path.join(appsRoot, "src", "lib", "api", "transport-web-commands", "shared.ts"),
];

function rewriteImports(outputText) {
  return outputText
    .replaceAll('./transport-web-commands/account', './transport-web-commands/account.js')
    .replaceAll('./transport-web-commands/aggregate-api', './transport-web-commands/aggregate-api.js')
    .replaceAll('./transport-web-commands/apikey', './transport-web-commands/apikey.js')
    .replaceAll('./transport-web-commands/browser-direct', './transport-web-commands/browser-direct.js')
    .replaceAll('./transport-web-commands/codex-profile', './transport-web-commands/codex-profile.js')
    .replaceAll('./transport-web-commands/codex-skills', './transport-web-commands/codex-skills.js')
    .replaceAll('./transport-web-commands/gateway', './transport-web-commands/gateway.js')
    .replaceAll('./transport-web-commands/login', './transport-web-commands/login.js')
    .replaceAll('./transport-web-commands/misc', './transport-web-commands/misc.js')
    .replaceAll('./transport-web-commands/proxy-profiles', './transport-web-commands/proxy-profiles.js')
    .replaceAll('./transport-web-commands/quota', './transport-web-commands/quota.js')
    .replaceAll('./transport-web-commands/shared', './transport-web-commands/shared.js')
    .replaceAll('./shared', './shared.js')
    .replaceAll('./browser-direct', './browser-direct.js')
    .replaceAll('../../utils/request', '../../utils/request.js');
}

async function writeCompiledModule(inputPath, outputPath) {
  const source = await fs.readFile(inputPath, "utf8");
  const compiled = ts.transpileModule(source, {
    compilerOptions: {
      module: ts.ModuleKind.ES2022,
      target: ts.ScriptTarget.ES2022,
    },
    fileName: inputPath,
  });
  await fs.mkdir(path.dirname(outputPath), { recursive: true });
  await fs.writeFile(outputPath, rewriteImports(compiled.outputText), "utf8");
}

async function ensureRequestUtils(tempDir) {
  const requestTempFile = path.join(tempDir, "utils", "request.js");
  await fs.mkdir(path.dirname(requestTempFile), { recursive: true });
  await fs.writeFile(
    requestTempFile,
    'export async function fetchWithRetry() { throw new Error("not used in this test"); }\nexport async function runWithControl(fn) { return await fn(); }\n',
    "utf8",
  );
}

async function loadTransportWebCommandsModule() {
  const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "codexmanager-transport-web-commands-"));
  await fs.writeFile(
    path.join(tempDir, "package.json"),
    '{"type":"module"}\n',
    "utf8",
  );
  const tempFile = path.join(tempDir, "transport-web-commands.mjs");
  await writeCompiledModule(sourcePath, tempFile);
  for (const modulePath of modulePaths) {
    const outputPath = path.join(tempDir, "transport-web-commands", `${path.basename(modulePath, ".ts")}.js`);
    await writeCompiledModule(modulePath, outputPath);
  }
  await ensureRequestUtils(tempDir);
  return import(pathToFileURL(tempFile).href);
}

const transportWebCommands = await loadTransportWebCommandsModule();
const commandMap = transportWebCommands.createWebCommandMap(async () => ({}));

test("quota reset warmup uses a dedicated batch setting RPC", () => {
  assert.deepEqual(commandMap.service_account_reset_warmup_update, {
    rpcMethod: "account/resetWarmup/update",
  });
});

test("createWebCommandMap keeps app and gateway transport settings payloads aligned", () => {
  const appSettingsSet = commandMap.app_settings_set;
  assert.equal(appSettingsSet.rpcMethod, "appSettings/set");
  assert.ok(appSettingsSet.mapParams);
  assert.deepEqual(
    appSettingsSet.mapParams({
      patch: { sseKeepaliveEnabled: false },
    }),
    { sseKeepaliveEnabled: false }
  );

  assert.deepEqual(commandMap.service_gateway_transport_set, {
    rpcMethod: "gateway/transport/set",
  });
});

test("createWebCommandMap 复用 keyId 到 id 的参数映射", () => {
  const descriptor = commandMap.service_apikey_delete;
  assert.ok(descriptor.mapParams);
  assert.deepEqual(descriptor.mapParams({ keyId: "key-1", extra: 1 }), {
    keyId: "key-1",
    extra: 1,
    id: "key-1",
  });
});

test("API key update Web 映射移除桌面 presence marker 并保留显式清空", () => {
  const descriptor = commandMap.service_apikey_update_model;
  assert.ok(descriptor.mapParams);
  assert.deepEqual(
    descriptor.mapParams({
      keyId: "key-1",
      hasName: true,
      name: "renamed",
      hasModelConfig: true,
      modelSlug: null,
      reasoningEffort: "high",
      serviceTier: null,
      hasRoutingConfig: true,
      rotationStrategy: "hybrid_rotation",
      aggregateApiId: null,
      accountPlanFilter: "plus",
      hasAccountGroupFilter: true,
      accountGroupFilter: null,
      hasQuotaLimitTokens: true,
      quotaLimitTokens: null,
    }),
    {
      keyId: "key-1",
      id: "key-1",
      name: "renamed",
      modelSlug: null,
      reasoningEffort: "high",
      serviceTier: null,
      rotationStrategy: "hybrid_rotation",
      aggregateApiId: null,
      accountPlanFilter: "plus",
      accountGroupFilter: null,
      quotaLimitTokens: null,
    },
  );
});

test("createWebCommandMap 为登录命令补齐 Web 运行壳参数", () => {
  const startLogin = commandMap.service_login_start;
  assert.ok(startLogin.mapParams);
  assert.deepEqual(startLogin.mapParams({ loginType: "chatgpt" }), {
    loginType: "chatgpt",
    type: "chatgpt",
    openBrowser: false,
  });
  assert.deepEqual(
    startLogin.mapParams({
      loginType: "chatgptDeviceCode",
      openBrowser: true,
    }),
    {
      loginType: "chatgptDeviceCode",
      type: "chatgptDeviceCode",
      openBrowser: false,
    },
  );

  assert.deepEqual(commandMap.service_login_cancel, {
    rpcMethod: "account/login/cancel",
  });

  const authTokens = commandMap.service_login_chatgpt_auth_tokens;
  assert.ok(authTokens.mapParams);
  assert.deepEqual(authTokens.mapParams({ foo: "bar" }), {
    foo: "bar",
    type: "chatgptAuthTokens",
  });
});

test("createWebCommandMap 为账号预热命令提供 Web RPC 映射", () => {
  const warmup = commandMap.service_account_warmup;
  assert.deepEqual(warmup, {
    rpcMethod: "account/warmup",
  });
});

test("createWebCommandMap 为批量账号排序提供 Web RPC 映射", () => {
  assert.deepEqual(commandMap.service_account_update_sorts, {
    rpcMethod: "account/updateSorts",
  });
});

test("createWebCommandMap 为账号模型拉取与关联提供 Web RPC 映射", () => {
  assert.deepEqual(commandMap.service_account_fetch_models, {
    rpcMethod: "account/fetchModels",
  });
  assert.deepEqual(commandMap.service_account_associate_models, {
    rpcMethod: "account/associateModels",
  });
});

test("createWebCommandMap 为额度重置查询和消费提供 Web RPC 映射", () => {
  assert.deepEqual(commandMap.service_usage_reset_credits, {
    rpcMethod: "account/usage/resetCredits",
  });
  assert.deepEqual(commandMap.service_usage_reset_credit_consume, {
    rpcMethod: "account/usage/resetCredit/consume",
  });
});

test("createWebCommandMap 为 Codex profile 管理提供 Web RPC 映射", () => {
  assert.deepEqual(commandMap.service_codex_profile_get, {
    rpcMethod: "codexProfile/get",
  });
  assert.deepEqual(commandMap.service_codex_profile_set_config, {
    rpcMethod: "codexProfile/setConfig",
  });
  assert.deepEqual(commandMap.service_codex_profile_list_candidates, {
    rpcMethod: "codexProfile/listCandidates",
  });
  assert.deepEqual(commandMap.service_codex_profile_apply_direct_account, {
    rpcMethod: "codexProfile/applyDirectAccount",
  });
  assert.deepEqual(commandMap.service_codex_profile_apply_direct_aggregate, {
    rpcMethod: "codexProfile/applyDirectAggregate",
  });
  assert.deepEqual(commandMap.service_codex_profile_apply_gateway, {
    rpcMethod: "codexProfile/applyGateway",
  });
  assert.deepEqual(commandMap.service_codex_profile_apply_models, {
    rpcMethod: "codexProfile/applyModels",
  });
  assert.deepEqual(commandMap.service_codex_profile_restore, {
    rpcMethod: "codexProfile/restore",
  });
  assert.deepEqual(commandMap.service_codex_profile_repair_history, {
    rpcMethod: "codexProfile/repairHistory",
  });
  assert.deepEqual(commandMap.service_codex_profile_prune_history_backups, {
    rpcMethod: "codexProfile/pruneHistoryBackups",
  });
});

test("createWebCommandMap 为 Codex Skills 管理提供 Web RPC 映射", () => {
  assert.deepEqual(commandMap.service_codex_skills_list, {
    rpcMethod: "codexSkills/list",
  });
  assert.deepEqual(commandMap.service_codex_skills_install_zip, {
    rpcMethod: "codexSkills/installZip",
  });
  assert.deepEqual(commandMap.service_codex_skills_import_directory, {
    rpcMethod: "codexSkills/importDirectory",
  });
  assert.deepEqual(commandMap.service_codex_skills_delete, {
    rpcMethod: "codexSkills/delete",
  });
  assert.deepEqual(commandMap.service_codex_skills_repository_list, {
    rpcMethod: "codexSkills/repositoryList",
  });
  assert.deepEqual(commandMap.service_codex_skills_repository_add, {
    rpcMethod: "codexSkills/repositoryAdd",
  });
  assert.deepEqual(commandMap.service_codex_skills_repository_delete, {
    rpcMethod: "codexSkills/repositoryDelete",
  });
  assert.deepEqual(commandMap.service_codex_skills_repository_refresh, {
    rpcMethod: "codexSkills/repositoryRefresh",
  });
  assert.deepEqual(commandMap.service_codex_skills_repository_install, {
    rpcMethod: "codexSkills/repositoryInstall",
  });
  assert.deepEqual(commandMap.service_codex_skills_registry_search, {
    rpcMethod: "codexSkills/registrySearch",
  });
  assert.deepEqual(commandMap.service_codex_skills_registry_install, {
    rpcMethod: "codexSkills/registryInstall",
  });
  assert.deepEqual(commandMap.service_codex_skills_marketplace_list, {
    rpcMethod: "codexSkills/marketplaceList",
  });
  assert.deepEqual(commandMap.service_codex_skills_marketplace_add, {
    rpcMethod: "codexSkills/marketplaceAdd",
  });
  assert.deepEqual(commandMap.service_codex_skills_marketplace_refresh, {
    rpcMethod: "codexSkills/marketplaceRefresh",
  });
  assert.deepEqual(
    commandMap.service_codex_skills_marketplace_plugin_install,
    { rpcMethod: "codexSkills/marketplacePluginInstall" },
  );
});

test("createWebCommandMap 为按状态清理账号提供 Web RPC 映射", () => {
  const cleanup = commandMap.service_account_delete_by_statuses;
  assert.deepEqual(cleanup, {
    rpcMethod: "account/deleteByStatuses",
  });
});

test("createWebCommandMap 为 system proxy profiles 提供 Web RPC 映射", () => {
  assert.deepEqual(commandMap.service_system_proxy_list, {
    rpcMethod: "system/proxy/list",
  });
  assert.deepEqual(commandMap.service_system_proxy_create, {
    rpcMethod: "system/proxy/create",
  });
  assert.deepEqual(commandMap.service_system_proxy_update, {
    rpcMethod: "system/proxy/update",
  });
  assert.deepEqual(commandMap.service_system_proxy_delete, {
    rpcMethod: "system/proxy/delete",
  });
  assert.deepEqual(commandMap.service_system_proxy_test_presets, {
    rpcMethod: "system/proxy/test-presets",
  });
  assert.deepEqual(commandMap.service_system_proxy_test_latency, {
    rpcMethod: "system/proxy/test-latency",
  });
  assert.deepEqual(commandMap.service_system_proxy_speed_test, {
    rpcMethod: "system/proxy/speed-test",
  });
  assert.deepEqual(commandMap.service_system_proxy_test_job, {
    rpcMethod: "system/proxy/test-job",
  });
  assert.deepEqual(commandMap.service_system_proxy_cancel_test, {
    rpcMethod: "system/proxy/cancel-test",
  });
  assert.deepEqual(commandMap.service_system_proxy_cloudflare_speed_test, {
    rpcMethod: "system/proxy/cloudflare-speed-test",
  });
});

test("createWebCommandMap 为 account proxy cloudflare speed test 提供 Web RPC 映射", () => {
  assert.deepEqual(commandMap.service_account_proxy_cloudflare_speed_test, {
    rpcMethod: "account/proxy/cloudflare-speed-test",
  });
});

test("createWebCommandMap 为显示主窗口提供 Web 回退", async () => {
  const previousWindow = globalThis.window;
  const location = { href: "/tray-preview/" };
  globalThis.window = { location };

  try {
    const showMainWindow = commandMap.app_show_main_window;
    assert.ok(showMainWindow.direct);
    assert.deepEqual(await showMainWindow.direct(), { ok: true });
    assert.equal(location.href, "/");
  } finally {
    if (previousWindow === undefined) {
      delete globalThis.window;
    } else {
      globalThis.window = previousWindow;
    }
  }
});

test("createWebCommandMap 为普通用户仪表盘汇总提供 Web RPC 映射", () => {
  const summary = commandMap.service_dashboard_member_summary;
  assert.equal(summary.rpcMethod, "dashboard/memberSummary");
  assert.ok(summary.mapParams);
  assert.deepEqual(
    summary.mapParams({
      user_id: "usr-1",
      day_start_ts: 100,
      day_end_ts: 200,
      include_details: false,
    }),
    { userId: "usr-1", dayStartTs: 100, dayEndTs: 200, includeDetails: false },
  );
});

test("createWebCommandMap 为管理员用量分析提供 Web RPC 映射", () => {
  const summary = commandMap.service_dashboard_admin_usage_summary;
  assert.equal(summary.rpcMethod, "dashboard/adminUsageSummary");
  assert.ok(summary.mapParams);
  assert.deepEqual(
    summary.mapParams({
      start_ts: 100,
      end_ts: 200,
      include_breakdowns: false,
      include_series: true,
      series_bucket_seconds: 3_600,
    }),
    {
      startTs: 100,
      endTs: 200,
      includeBreakdowns: false,
      includeSeries: true,
      seriesBucketSeconds: 3_600,
    },
  );
});

test("createWebCommandMap 为模型目录 V2 原子命令提供 Web RPC 映射", () => {
  assert.deepEqual(commandMap.service_managed_model_list_v2, {
    rpcMethod: "apikey/managedModelListV2",
  });
  assert.deepEqual(commandMap.service_managed_model_get_v2, {
    rpcMethod: "apikey/managedModelGetV2",
  });
  assert.deepEqual(commandMap.service_managed_model_delete_v2, {
    rpcMethod: "apikey/managedModelDeleteV2",
  });
  const priceSync = commandMap.service_managed_model_price_sync_v2;
  assert.equal(priceSync.rpcMethod, "apikey/managedModelPriceSyncV2");
  assert.ok(priceSync.mapParams);
  assert.deepEqual(
    priceSync.mapParams({
      addr: "localhost:48760",
      payload: { modelSlugs: ["gpt-6-sol", "gpt-6-luna"] },
    }),
    { modelSlugs: ["gpt-6-sol", "gpt-6-luna"] },
  );

  const upsert = commandMap.service_managed_model_upsert_v2;
  assert.equal(upsert.rpcMethod, "apikey/managedModelUpsertV2");
  assert.ok(upsert.mapParams);
  assert.deepEqual(
    upsert.mapParams({ payload: { previousSlug: null, model: { slug: "local-x" } } }),
    { previousSlug: null, model: { slug: "local-x" } },
  );

  const updateState = commandMap.service_managed_model_update_state_v2;
  assert.equal(updateState.rpcMethod, "apikey/managedModelUpdateStateV2");
  assert.ok(updateState.mapParams);
  assert.deepEqual(
    updateState.mapParams({
      payload: { slug: "gpt-5.4", enabled: false, visibility: "hide" },
    }),
    { slug: "gpt-5.4", enabled: false, visibility: "hide" },
  );

  const batchUpdateState =
    commandMap.service_managed_model_batch_update_state_v2;
  assert.equal(
    batchUpdateState.rpcMethod,
    "apikey/managedModelBatchUpdateStateV2",
  );
  assert.ok(batchUpdateState.mapParams);
  assert.deepEqual(
    batchUpdateState.mapParams({
      payload: {
        slugs: ["gpt-5.4", "gpt-5.4-mini"],
        enabled: true,
        visibility: "list",
      },
    }),
    {
      slugs: ["gpt-5.4", "gpt-5.4-mini"],
      enabled: true,
      visibility: "list",
    },
  );

  for (const command of [
    "service_managed_model_import_preview_v2",
    "service_managed_model_import_commit_v2",
  ]) {
    const descriptor = commandMap[command];
    assert.ok(descriptor.mapParams);
    assert.deepEqual(
      descriptor.mapParams({
        payload: { jsonContent: "{}", conflictStrategy: "keep_existing" },
      }),
      { jsonContent: "{}", conflictStrategy: "keep_existing" },
    );
  }
});

test("createWebCommandMap 不再暴露旧模型发现与供应商模型命令", () => {
  for (const command of [
    "service_model_routing",
    "service_model_source_sync",
    "service_model_source_mapping_save",
    "service_aggregate_api_supplier_model_save",
    "service_aggregate_api_supplier_models_import",
  ]) {
    assert.equal(commandMap[command], undefined);
  }
});

test("createWebCommandMap 为外部协议跳转提供当前窗口回退", async () => {
  const previousWindow = globalThis.window;
  const location = { href: "/" };
  globalThis.window = { location };

  try {
    const openExternalUrl = commandMap.open_external_url;
    assert.ok(openExternalUrl.direct);
    assert.deepEqual(await openExternalUrl.direct({ url: " ccswitch://v1/import?resource=provider " }), { ok: true });
    assert.equal(location.href, "ccswitch://v1/import?resource=provider");
  } finally {
    if (previousWindow === undefined) {
      delete globalThis.window;
    } else {
      globalThis.window = previousWindow;
    }
  }
});
