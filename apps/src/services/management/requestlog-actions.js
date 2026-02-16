export function createRequestLogActions({
  dom,
  state,
  ensureConnected,
  withButtonBusy,
  showToast,
  showConfirmDialog,
  clearRequestLogs,
  refreshRequestLogs,
  renderRequestLogs,
}) {
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
      try {
        const res = await clearRequestLogs();
        if (res && res.ok === false) {
          showToast(res.error || "清空日志失败", "error");
          return;
        }
        const applied = await refreshRequestLogs(state.requestLogQuery, { latestOnly: true });
        if (applied !== false) {
          renderRequestLogs();
        }
        showToast("请求日志已清空");
      } catch (err) {
        showToast(`清空日志失败：${err instanceof Error ? err.message : String(err)}`, "error");
      }
    });
  }

  return { handleClearRequestLogs };
}
