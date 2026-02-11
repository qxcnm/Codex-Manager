let apiModelLoadSeq = 0;
let requestLogSearchTimer = null;

export function bindMainEvents({
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
  handleServiceToggle,
  renderAccountsView,
  updateRequestLogFilterButtons,
}) {
  dom.navDashboard.addEventListener("click", () => switchPage("dashboard"));
  dom.navAccounts.addEventListener("click", () => switchPage("accounts"));
  dom.navApiKeys.addEventListener("click", () => switchPage("apikeys"));
  dom.navRequestLogs.addEventListener("click", () => switchPage("requestlogs"));
  dom.addAccountBtn.addEventListener("click", openAccountModal);
  dom.createApiKeyBtn.addEventListener("click", async () => {
    openApiKeyModal();
    // 中文注释：先用本地缓存秒开；仅在模型列表为空时再后台懒加载，避免弹窗开关被网络拖慢。
    if (state.apiModelOptions && state.apiModelOptions.length > 0) {
      return;
    }
    const currentSeq = ++apiModelLoadSeq;
    const ok = await ensureConnected();
    if (!ok || currentSeq !== apiModelLoadSeq) return;
    await refreshApiModels();
    if (currentSeq !== apiModelLoadSeq) return;
    if (!dom.modalApiKey || !dom.modalApiKey.classList.contains("active")) return;
    populateApiKeyModelSelect();
  });
  if (dom.closeAccountModal) {
    dom.closeAccountModal.addEventListener("click", closeAccountModal);
  }
  dom.cancelLogin.addEventListener("click", closeAccountModal);
  dom.submitLogin.addEventListener("click", handleLogin);
  dom.copyLoginUrl.addEventListener("click", () => {
    if (!dom.loginUrl.value) return;
    dom.loginUrl.select();
    dom.loginUrl.setSelectionRange(0, dom.loginUrl.value.length);
    try {
      document.execCommand("copy");
      showToast("授权链接已复制");
    } catch (err) {
      showToast("复制失败，请手动复制链接", "error");
    }
  });
  dom.manualCallbackSubmit.addEventListener("click", handleManualCallback);
  dom.closeUsageModal.addEventListener("click", closeUsageModal);
  dom.refreshUsageSingle.addEventListener("click", refreshUsageForAccount);
  if (dom.closeApiKeyModal) {
    dom.closeApiKeyModal.addEventListener("click", closeApiKeyModal);
  }
  dom.cancelApiKey.addEventListener("click", closeApiKeyModal);
  dom.submitApiKey.addEventListener("click", createApiKey);
  dom.copyApiKey.addEventListener("click", () => {
    if (!dom.apiKeyValue.value) return;
    dom.apiKeyValue.select();
    dom.apiKeyValue.setSelectionRange(0, dom.apiKeyValue.value.length);
    try {
      document.execCommand("copy");
      showToast("平台 Key 已复制");
    } catch (_err) {
      showToast("复制失败，请手动复制", "error");
    }
  });
  if (dom.refreshRequestLogs) {
    dom.refreshRequestLogs.addEventListener("click", async () => {
      await refreshRequestLogs(state.requestLogQuery);
      renderRequestLogs();
    });
  }
  if (dom.clearRequestLogs) {
    dom.clearRequestLogs.addEventListener("click", handleClearRequestLogs);
  }
  if (dom.requestLogSearch) {
    dom.requestLogSearch.addEventListener("input", (event) => {
      state.requestLogQuery = event.target.value || "";
      if (requestLogSearchTimer) {
        clearTimeout(requestLogSearchTimer);
      }
      requestLogSearchTimer = setTimeout(async () => {
        await refreshRequestLogs(state.requestLogQuery);
        renderRequestLogs();
      }, 220);
    });
  }
  const setLogFilter = (value) => {
    state.requestLogStatusFilter = value;
    updateRequestLogFilterButtons();
    renderRequestLogs();
  };
  if (dom.filterLogAll) dom.filterLogAll.addEventListener("click", () => setLogFilter("all"));
  if (dom.filterLog2xx) dom.filterLog2xx.addEventListener("click", () => setLogFilter("2xx"));
  if (dom.filterLog4xx) dom.filterLog4xx.addEventListener("click", () => setLogFilter("4xx"));
  if (dom.filterLog5xx) dom.filterLog5xx.addEventListener("click", () => setLogFilter("5xx"));
  if (dom.refreshAll) {
    dom.refreshAll.addEventListener("click", refreshAll);
  }
  if (dom.inputApiKeyModel && dom.inputApiKeyReasoning) {
    const syncReasoningSelect = () => {
      const enabled = Boolean((dom.inputApiKeyModel.value || "").trim());
      dom.inputApiKeyReasoning.disabled = !enabled;
      if (!enabled) {
        dom.inputApiKeyReasoning.value = "";
      }
    };
    dom.inputApiKeyModel.addEventListener("change", syncReasoningSelect);
    syncReasoningSelect();
  }
  if (dom.themeToggle) {
    dom.themeToggle.addEventListener("click", (event) => {
      event.stopPropagation();
      toggleThemePanel();
    });
  }
  if (dom.themePanel) {
    dom.themePanel.addEventListener("click", (event) => {
      const target = event.target;
      if (target instanceof HTMLElement) {
        const themeButton = target.closest("button[data-theme]");
        if (themeButton && themeButton.dataset.theme) {
          setTheme(themeButton.dataset.theme);
          closeThemePanel();
        }
      }
      event.stopPropagation();
    });
  }
  document.addEventListener("click", () => closeThemePanel());
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape") closeThemePanel();
  });
  dom.serviceToggleBtn.addEventListener("click", handleServiceToggle);

  if (dom.accountSearch) {
    dom.accountSearch.addEventListener("input", (event) => {
      state.accountSearch = event.target.value;
      renderAccountsView();
    });
  }

  const updateFilterButtons = () => {
    if (dom.filterAll) dom.filterAll.classList.toggle("active", state.accountFilter === "all");
    if (dom.filterActive) dom.filterActive.classList.toggle("active", state.accountFilter === "active");
    if (dom.filterLow) dom.filterLow.classList.toggle("active", state.accountFilter === "low");
  };

  const setFilter = (filter) => {
    state.accountFilter = filter;
    updateFilterButtons();
    renderAccountsView();
  };

  if (dom.filterAll) dom.filterAll.addEventListener("click", () => setFilter("all"));
  if (dom.filterActive) dom.filterActive.addEventListener("click", () => setFilter("active"));
  if (dom.filterLow) dom.filterLow.addEventListener("click", () => setFilter("low"));
}

