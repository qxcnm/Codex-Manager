import * as api from "../../api";

export function createAccountActions({
  ensureConnected,
  refreshAll,
  showToast,
  showConfirmDialog,
}) {
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

  return { updateAccountSort, deleteAccount };
}
