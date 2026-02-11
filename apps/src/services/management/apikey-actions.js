import * as api from "../../api";

export function createApiKeyActions({
  dom,
  ensureConnected,
  withButtonBusy,
  showToast,
  showConfirmDialog,
  refreshApiModels,
  refreshApiKeys,
  populateApiKeyModelSelect,
  renderApiKeys,
}) {
  let actions = null;

  const renderApiKeyList = () => {
    renderApiKeys({
      onToggleStatus: actions.toggleApiKeyStatus,
      onDelete: actions.deleteApiKey,
      onUpdateModel: actions.updateApiKeyModel,
    });
  };

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
      renderApiKeyList();
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
    renderApiKeyList();
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
    renderApiKeyList();
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
    renderApiKeyList();
  }

  actions = { createApiKey, deleteApiKey, toggleApiKeyStatus, updateApiKeyModel };
  return actions;
}


