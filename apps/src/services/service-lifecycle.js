export function createServiceLifecycle({
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
}) {
  function updateServiceToggle() {
    if (!dom.serviceToggleBtn) return;
    if (state.serviceBusy) return;
    dom.serviceToggleBtn.textContent = state.serviceConnected ? "停止服务" : "启动服务";
  }

  function setServiceBusy(busy, mode) {
    state.serviceBusy = busy;
    if (!dom.serviceToggleBtn) return;
    dom.serviceToggleBtn.disabled = busy;
    dom.serviceToggleBtn.classList.toggle("is-loading", busy);
    if (busy) {
      dom.serviceToggleBtn.textContent = mode === "stop" ? "停止中..." : "启动中...";
    } else {
      updateServiceToggle();
    }
  }

  function syncServiceAddrFromInput() {
    if (!dom.serviceAddrInput) return;
    const raw = dom.serviceAddrInput.value;
    if (!raw) return;
    try {
      state.serviceAddr = normalizeAddr(raw);
    } catch (err) {
      // ignore invalid input during bootstrap
    }
  }

  function restoreServiceAddr() {
    const savedAddr = localStorage.getItem("gpttools.service.addr");
    if (savedAddr) {
      state.serviceAddr = savedAddr;
      dom.serviceAddrInput.value = savedAddr;
      syncServiceAddrFromInput();
      return;
    }
    dom.serviceAddrInput.value = "48760";
    syncServiceAddrFromInput();
  }

  async function handleStartService() {
    setServiceBusy(true, "start");
    const started = await startService(dom.serviceAddrInput.value, {
      skipInitialize: true,
    });
    dom.serviceAddrInput.value = state.serviceAddr;
    localStorage.setItem("gpttools.service.addr", state.serviceAddr);
    if (!started) {
      setServiceBusy(false);
      updateServiceToggle();
      return;
    }
    const probeId = state.serviceProbeId + 1;
    state.serviceProbeId = probeId;
    void waitForConnection({ retries: 12, delayMs: 400, silent: true }).then(
      (ok) => {
        if (state.serviceProbeId !== probeId) return;
        setServiceBusy(false);
        updateServiceToggle();
        if (!ok) {
          const reason = state.serviceLastError ? `：${state.serviceLastError}` : "";
          setServiceHint(`连接失败${reason}，请检查端口或 service 状态`, true);
          return;
        }
        void refreshAll();
        ensureAutoRefreshTimer(state, refreshAll);
      },
    );
  }

  async function handleStopService() {
    setServiceBusy(true, "stop");
    state.serviceProbeId += 1;
    await stopService();
    setServiceBusy(false);
    updateServiceToggle();
    stopAutoRefreshTimer(state);
  }

  async function handleServiceToggle() {
    if (state.serviceBusy) return;
    if (state.serviceConnected) {
      await handleStopService();
    } else {
      await handleStartService();
    }
  }

  async function autoStartService() {
    if (!dom.serviceAddrInput) return;
    syncServiceAddrFromInput();
    const probeId = state.serviceProbeId + 1;
    state.serviceProbeId = probeId;
    const ok = await waitForConnection({
      retries: 1,
      delayMs: 200,
      silent: true,
    });
    if (state.serviceProbeId !== probeId) return;
    if (ok) {
      updateServiceToggle();
      void refreshAll();
      // 中文注释：探活成功后立即复用统一定时器入口，避免“已连通但未启动自动刷新”的状态分叉。
      ensureAutoRefreshTimer(state, refreshAll);
      return;
    }
    await handleStartService();
  }

  return {
    updateServiceToggle,
    restoreServiceAddr,
    autoStartService,
    handleServiceToggle,
  };
}
