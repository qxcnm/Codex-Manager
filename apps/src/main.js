import "./styles/base.css";
import "./styles/layout.css";
import "./styles/components.css";
import "./styles/responsive.css";

import { state } from "./state";
import { dom } from "./ui/dom";
import { setStatus, setServiceHint } from "./ui/status";
import * as api from "./api";
import {
  ensureConnected,
  normalizeAddr,
  startService,
  stopService,
  waitForConnection,
} from "./services/connection";
import {
  refreshAccounts,
  refreshUsageList,
  refreshApiKeys,
  refreshApiModels,
  refreshRequestLogs,
  clearRequestLogs,
} from "./services/data";
import {
  ensureAutoRefreshTimer,
  runRefreshTasks,
  stopAutoRefreshTimer,
} from "./services/refresh";
import { createServiceLifecycle } from "./services/service-lifecycle";
import { openAccountModal, closeAccountModal } from "./views/accounts";
import { renderApiKeys, openApiKeyModal, closeApiKeyModal, populateApiKeyModelSelect } from "./views/apikeys";
import { openUsageModal, closeUsageModal, renderUsageSnapshot } from "./views/usage";
import { renderRequestLogs } from "./views/requestlogs";
import { renderAllViews, renderAccountsOnly } from "./views/renderers";
import { bindMainEvents } from "./views/event-bindings";

let toastTimer = null;
let toastQueue = [];
let toastActive = false;

function switchPage(page) {
  state.currentPage = page;
  closeThemePanel();
  dom.navDashboard.classList.toggle("active", page === "dashboard");
  dom.navAccounts.classList.toggle("active", page === "accounts");
  dom.navApiKeys.classList.toggle("active", page === "apikeys");
  dom.navRequestLogs.classList.toggle("active", page === "requestlogs");
  dom.pageDashboard.classList.toggle("active", page === "dashboard");
  dom.pageAccounts.classList.toggle("active", page === "accounts");
  dom.pageApiKeys.classList.toggle("active", page === "apikeys");
  dom.pageRequestLogs.classList.toggle("active", page === "requestlogs");
  dom.pageTitle.textContent =
    page === "dashboard"
      ? "仪表盘"
      : page === "accounts"
        ? "账号管理"
        : page === "apikeys"
          ? "平台 Key"
          : "请求日志";
}

function updateRequestLogFilterButtons() {
  const current = state.requestLogStatusFilter || "all";
  if (dom.filterLogAll) dom.filterLogAll.classList.toggle("active", current === "all");
  if (dom.filterLog2xx) dom.filterLog2xx.classList.toggle("active", current === "2xx");
  if (dom.filterLog4xx) dom.filterLog4xx.classList.toggle("active", current === "4xx");
  if (dom.filterLog5xx) dom.filterLog5xx.classList.toggle("active", current === "5xx");
}

async function withButtonBusy(button, busyText, task) {
  if (!button) {
    return task();
  }
  if (button.dataset.busy === "1") {
    return;
  }
  const originalText = button.textContent;
  button.dataset.busy = "1";
  button.disabled = true;
  button.classList.add("is-loading");
  if (busyText) {
    button.textContent = busyText;
  }
  try {
    return await task();
  } finally {
    button.dataset.busy = "0";
    button.disabled = false;
    button.classList.remove("is-loading");
    button.textContent = originalText;
  }
}

function showToast(message, type = "info") {
  if (!message) return;
  if (!dom.appToast) {
    return;
  }
  toastQueue.push({ message: String(message), type });
  if (toastActive) return;
  const flushNext = () => {
    const item = toastQueue.shift();
    if (!item) {
      toastActive = false;
      return;
    }
    toastActive = true;
    dom.appToast.textContent = item.message;
    dom.appToast.classList.toggle("is-error", item.type === "error");
    dom.appToast.classList.add("active");
    if (toastTimer) {
      clearTimeout(toastTimer);
    }
    toastTimer = setTimeout(() => {
      dom.appToast.classList.remove("active");
      setTimeout(flushNext, 180);
    }, 2400);
  };
  flushNext();
}

function showConfirmDialog({
  title = "确认操作",
  message = "请确认是否继续。",
  confirmText = "确定",
  cancelText = "取消",
} = {}) {
  if (
    !dom.modalConfirm
    || !dom.confirmTitle
    || !dom.confirmMessage
    || !dom.confirmOk
    || !dom.confirmCancel
  ) {
    return Promise.resolve(window.confirm(message));
  }
  dom.confirmTitle.textContent = title;
  dom.confirmMessage.textContent = message;
  dom.confirmOk.textContent = confirmText;
  dom.confirmCancel.textContent = cancelText;
  dom.modalConfirm.classList.add("active");
  return new Promise((resolve) => {
    let settled = false;
    const cleanup = () => {
      if (settled) return;
      settled = true;
      dom.confirmOk.removeEventListener("click", onOk);
      dom.confirmCancel.removeEventListener("click", onCancel);
      dom.modalConfirm.removeEventListener("click", onBackdropClick);
      document.removeEventListener("keydown", onKeydown);
      dom.modalConfirm.classList.remove("active");
    };
    const onOk = () => {
      cleanup();
      resolve(true);
    };
    const onCancel = () => {
      cleanup();
      resolve(false);
    };
    const onBackdropClick = (event) => {
      if (event.target === dom.modalConfirm) {
        onCancel();
      }
    };
    const onKeydown = (event) => {
      if (event.key === "Escape") {
        onCancel();
      }
    };
    dom.confirmOk.addEventListener("click", onOk, { once: true });
    dom.confirmCancel.addEventListener("click", onCancel, { once: true });
    dom.modalConfirm.addEventListener("click", onBackdropClick);
    document.addEventListener("keydown", onKeydown);
  });
}

const THEME_OPTIONS = [
  { id: "tech", label: "科技蓝" },
  { id: "business", label: "商务金" },
  { id: "mint", label: "薄荷绿" },
  { id: "sunset", label: "晚霞橙" },
  { id: "grape", label: "葡萄紫" },
  { id: "ocean", label: "海湾青" },
  { id: "forest", label: "松林绿" },
  { id: "rose", label: "玫瑰粉" },
  { id: "slate", label: "石板灰" },
  { id: "aurora", label: "极光青" },
];

function renderThemeButtons() {
  if (!dom.themePanel) return;
  dom.themePanel.innerHTML = "";
  THEME_OPTIONS.forEach((theme) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "secondary";
    button.dataset.theme = theme.id;
    button.textContent = theme.label;
    dom.themePanel.appendChild(button);
  });
}

function setTheme(theme) {
  const validThemes = new Set(THEME_OPTIONS.map((item) => item.id));
  const nextTheme = validThemes.has(theme) ? theme : "tech";
  document.body.dataset.theme = nextTheme;
  localStorage.setItem("gpttools.ui.theme", nextTheme);
  if (dom.themePanel) {
    dom.themePanel.querySelectorAll("button[data-theme]").forEach((button) => {
      button.classList.toggle("is-active", button.dataset.theme === nextTheme);
    });
  }
  if (dom.themeToggle) {
    const activeTheme = THEME_OPTIONS.find((item) => item.id === nextTheme);
    dom.themeToggle.textContent = activeTheme ? `主题 · ${activeTheme.label}` : "主题";
  }
}

function restoreTheme() {
  const savedTheme = localStorage.getItem("gpttools.ui.theme");
  setTheme(savedTheme || "tech");
}

function closeThemePanel() {
  if (!dom.themePanel || !dom.themeToggle) return;
  dom.themePanel.hidden = true;
  dom.themeToggle.setAttribute("aria-expanded", "false");
}

function openThemePanel() {
  if (!dom.themePanel || !dom.themeToggle) return;
  dom.themePanel.hidden = false;
  dom.themeToggle.setAttribute("aria-expanded", "true");
}

function toggleThemePanel() {
  if (!dom.themePanel) return;
  if (dom.themePanel.hidden) {
    openThemePanel();
  } else {
    closeThemePanel();
  }
}

async function refreshAll() {
  const ok = await ensureConnected();
  serviceLifecycle.updateServiceToggle();
  if (!ok) return;
  const results = await runRefreshTasks(
    [
      { name: "accounts", run: refreshAccounts },
      { name: "usage", run: refreshUsageList },
      { name: "api-models", run: refreshApiModels },
      { name: "api-keys", run: refreshApiKeys },
      { name: "request-logs", run: () => refreshRequestLogs(state.requestLogQuery) },
    ],
    (taskName, err) => {
      console.error(`[refreshAll] ${taskName} failed`, err);
    },
  );
  // 中文注释：并行刷新时允许“部分失败部分成功”，否则某个慢/失败接口会拖垮整页刷新体验。
  const hasFailedTask = results.some((item) => item.status === "rejected");
  if (hasFailedTask) {
    showToast("部分数据刷新失败，已展示可用数据", "error");
  }
  renderAllViews({
    onUpdateSort: updateAccountSort,
    onOpenUsage: handleOpenUsageModal,
    onDeleteAccount: deleteAccount,
    onToggleApiKeyStatus: toggleApiKeyStatus,
    onDeleteApiKey: deleteApiKey,
    onUpdateApiKeyModel: updateApiKeyModel,
  });
}

const serviceLifecycle = createServiceLifecycle({
  state,
  dom,
  setServiceHint,
  normalizeAddr,
  startService,
  stopService,
  waitForConnection,
  refreshAll,
  ensureAutoRefreshTimer,
  stopAutoRefreshTimer,
});

async function handleClearRequestLogs() {
  const confirmed = await showConfirmDialog({
    title: "清空请求日志",
    message: "确定清空请求日志吗？该操作不可撤销。",
    confirmText: "清空",
    cancelText: "取消",
  });
  if (!confirmed) return;
  await withButtonBusy(dom.clearRequestLogs, "清空中...", async () => {
    const ok = await ensureConnected();
    if (!ok) return;
    const res = await clearRequestLogs();
    if (res && res.ok === false) {
      showToast(res.error || "清空日志失败", "error");
      return;
    }
    await refreshRequestLogs(state.requestLogQuery);
    renderRequestLogs();
    showToast("请求日志已清空");
  });
}

async function updateAccountSort(accountId, sort) {
  const ok = await ensureConnected();
  if (!ok) return;
  await api.serviceAccountUpdate(accountId, sort);
  await refreshAll();
}

async function deleteAccount(account) {
  if (!account || !account.id) return;
  const confirmed = await showConfirmDialog({
    title: "删除账号",
    message: `确定删除账号 ${account.label} 吗？删除后不可恢复。`,
    confirmText: "删除",
    cancelText: "取消",
  });
  if (!confirmed) return;
  const ok = await ensureConnected();
  if (!ok) return;
  const res = await api.serviceAccountDelete(account.id);
  if (res && res.error === "unknown_method") {
    const fallback = await api.localAccountDelete(account.id);
    if (fallback && fallback.ok) {
      await refreshAll();
      return;
    }
    const msg = fallback && fallback.error ? fallback.error : "删除失败";
    showToast(msg, "error");
    return;
  }
  if (res && res.ok) {
    await refreshAll();
    showToast("账号已删除");
  } else {
    const msg = res && res.error ? res.error : "删除失败";
    showToast(msg, "error");
  }
}

async function handleOpenUsageModal(account) {
  openUsageModal(account);
  await refreshUsageForAccount();
}

async function refreshUsageForAccount() {
  if (!state.currentUsageAccount) return;
  const ok = await ensureConnected();
  if (!ok) return;
  dom.refreshUsageSingle.disabled = true;
  try {
    await api.serviceUsageRefresh(state.currentUsageAccount.id);
    const res = await api.serviceUsageRead(state.currentUsageAccount.id);
    const snap = res ? res.snapshot : null;
    renderUsageSnapshot(snap);
  } catch (err) {
    dom.usageDetail.textContent = String(err);
  }
  dom.refreshUsageSingle.disabled = false;
}

async function createApiKey() {
  await withButtonBusy(dom.submitApiKey, "创建中...", async () => {
    const ok = await ensureConnected();
    if (!ok) return;
    const modelSlug = dom.inputApiKeyModel.value || null;
    const reasoningEffort = modelSlug ? (dom.inputApiKeyReasoning.value || null) : null;
    const res = await api.serviceApiKeyCreate(
      dom.inputApiKeyName.value.trim() || null,
      modelSlug,
      reasoningEffort,
    );
    if (res && res.error) {
      showToast(res.error, "error");
      return;
    }
    dom.apiKeyValue.value = res && res.key ? res.key : "";
    await refreshApiModels();
    await refreshApiKeys();
    populateApiKeyModelSelect();
    renderApiKeys({
      onToggleStatus: toggleApiKeyStatus,
      onDelete: deleteApiKey,
      onUpdateModel: updateApiKeyModel,
    });
    showToast("平台 Key 创建成功");
  });
}

async function deleteApiKey(item) {
  if (!item || !item.id) return;
  const confirmed = await showConfirmDialog({
    title: "删除平台 Key",
    message: `确定删除平台 Key ${item.id} 吗？`,
    confirmText: "删除",
    cancelText: "取消",
  });
  if (!confirmed) return;
  const ok = await ensureConnected();
  if (!ok) return;
  await api.serviceApiKeyDelete(item.id);
  await refreshApiKeys();
  renderApiKeys({
    onToggleStatus: toggleApiKeyStatus,
    onDelete: deleteApiKey,
    onUpdateModel: updateApiKeyModel,
  });
  showToast("平台 Key 已删除");
}

async function toggleApiKeyStatus(item) {
  if (!item || !item.id) return;
  const ok = await ensureConnected();
  if (!ok) return;
  const isDisabled = String(item.status || "").toLowerCase() === "disabled";
  if (isDisabled) {
    await api.serviceApiKeyEnable(item.id);
  } else {
    await api.serviceApiKeyDisable(item.id);
  }
  await refreshApiKeys();
  renderApiKeys({
    onToggleStatus: toggleApiKeyStatus,
    onDelete: deleteApiKey,
    onUpdateModel: updateApiKeyModel,
  });
  showToast(isDisabled ? "平台 Key 已启用" : "平台 Key 已禁用");
}

async function updateApiKeyModel(item, modelSlug, reasoningEffort) {
  if (!item || !item.id) return;
  const ok = await ensureConnected();
  if (!ok) return;
  const normalizedModel = modelSlug || null;
  const normalizedEffort = normalizedModel ? (reasoningEffort || null) : null;
  const res = await api.serviceApiKeyUpdateModel(item.id, normalizedModel, normalizedEffort);
  if (res && res.ok === false) {
    showToast(res.error || "模型配置保存失败", "error");
    return;
  }
  await refreshApiKeys();
  renderApiKeys({
    onToggleStatus: toggleApiKeyStatus,
    onDelete: deleteApiKey,
    onUpdateModel: updateApiKeyModel,
  });
}

async function handleLogin() {
  await withButtonBusy(dom.submitLogin, "授权中...", async () => {
    const ok = await ensureConnected();
    if (!ok) return;
    dom.loginUrl.value = "生成授权链接中...";
    try {
      const res = await api.serviceLoginStart({
        loginType: "chatgpt",
        openBrowser: false,
        note: dom.inputNote.value.trim(),
        tags: dom.inputTags.value.trim(),
        groupName: dom.inputGroup.value.trim(),
      });
      if (res && res.error) {
        dom.loginHint.textContent = `登录失败：${res.error}`;
        dom.loginUrl.value = "";
        return;
      }
      dom.loginUrl.value = res && res.authUrl ? res.authUrl : "";
      if (res && res.authUrl) {
        await api.openInBrowser(res.authUrl);
        if (res.warning) {
          dom.loginHint.textContent = `注意：${res.warning}。如无法回调，可在下方粘贴回调链接手动解析。`;
        } else {
          dom.loginHint.textContent = "已打开浏览器，请完成授权。";
        }
      } else {
        dom.loginHint.textContent = "未获取到授权链接，请重试。";
      }
      state.activeLoginId = res && res.loginId ? res.loginId : null;
      const success = await waitForLogin(state.activeLoginId);
      if (success) {
        await refreshAll();
        closeAccountModal();
      } else {
        dom.loginHint.textContent = "登录失败，请重试。";
      }
    } catch (err) {
      dom.loginUrl.value = "";
      dom.loginHint.textContent = "登录失败，请检查 service 状态。";
    }
  });
}

function parseCallbackUrl(raw) {
  const value = String(raw || "").trim();
  if (!value) {
    return { error: "请粘贴回调链接" };
  }
  let url;
  try {
    url = new URL(value);
  } catch (err) {
    try {
      url = new URL(`http://${value}`);
    } catch (error) {
      return { error: "回调链接格式不正确" };
    }
  }
  const code = url.searchParams.get("code");
  const state = url.searchParams.get("state");
  if (!code || !state) {
    return { error: "回调链接缺少 code/state" };
  }
  const redirectUri = `${url.origin}${url.pathname}`;
  return { code, state, redirectUri };
}

async function handleManualCallback() {
  const parsed = parseCallbackUrl(dom.manualCallbackUrl.value);
  if (parsed.error) {
    dom.loginHint.textContent = parsed.error;
    return;
  }
  await withButtonBusy(dom.manualCallbackSubmit, "解析中...", async () => {
    const ok = await ensureConnected();
    if (!ok) return;
    dom.loginHint.textContent = "解析回调中...";
    try {
      const res = await api.serviceLoginComplete(
        parsed.state,
        parsed.code,
        parsed.redirectUri,
      );
      if (res && res.ok) {
        dom.loginHint.textContent = "登录成功，正在刷新...";
        await refreshAll();
        closeAccountModal();
        return;
      }
      const msg = res && res.error ? res.error : "解析失败";
      dom.loginHint.textContent = `登录失败：${msg}`;
    } catch (err) {
      dom.loginHint.textContent = `登录失败：${String(err)}`;
    }
  });
}

async function waitForLogin(loginId) {
  if (!loginId) return false;
  const deadline = Date.now() + 2 * 60 * 1000;
  while (Date.now() < deadline) {
    const res = await api.serviceLoginStatus(loginId);
    if (res && res.status === "success") return true;
    if (res && res.status === "failed") {
      dom.loginHint.textContent = `登录失败：${res.error || "unknown"}`;
      return false;
    }
    await new Promise((r) => setTimeout(r, 1500));
  }
  dom.loginHint.textContent = "登录超时，请重试。";
  return false;
}


function bindEvents() {
  bindMainEvents({
    dom,
    state,
    switchPage,
    openAccountModal,
    openApiKeyModal,
    closeAccountModal,
    handleLogin,
    showToast,
    handleManualCallback,
    closeUsageModal,
    refreshUsageForAccount,
    closeApiKeyModal,
    createApiKey,
    handleClearRequestLogs,
    refreshRequestLogs,
    renderRequestLogs,
    refreshAll,
    ensureConnected,
    refreshApiModels,
    populateApiKeyModelSelect,
    toggleThemePanel,
    closeThemePanel,
    setTheme,
    handleServiceToggle: serviceLifecycle.handleServiceToggle,
    renderAccountsOnly,
    updateAccountSort,
    handleOpenUsageModal,
    deleteAccount,
    updateRequestLogFilterButtons,
  });
}

function bootstrap() {
  setStatus("", false);
  setServiceHint("请输入端口并点击启动", false);
  renderThemeButtons();
  restoreTheme();
  serviceLifecycle.restoreServiceAddr();
  serviceLifecycle.updateServiceToggle();
  void serviceLifecycle.autoStartService();
  bindEvents();
  renderAllViews({
    onUpdateSort: updateAccountSort,
    onOpenUsage: handleOpenUsageModal,
    onDeleteAccount: deleteAccount,
    onToggleApiKeyStatus: toggleApiKeyStatus,
    onDeleteApiKey: deleteApiKey,
    onUpdateApiKeyModel: updateApiKeyModel,
  });
  updateRequestLogFilterButtons();
}

window.addEventListener("DOMContentLoaded", bootstrap);
