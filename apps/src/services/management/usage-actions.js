import * as api from "../../api";

export function createUsageActions({
  dom,
  state,
  ensureConnected,
  openUsageModal,
  renderUsageSnapshot,
}) {
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

  return { handleOpenUsageModal, refreshUsageForAccount };
}
