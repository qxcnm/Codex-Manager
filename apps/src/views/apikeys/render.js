import { state } from "../../state.js";
import { dom } from "../../ui/dom.js";
import {
  REASONING_OPTIONS,
  getProtocolProfileLabel,
  getStatusViewModel,
  mapReasoningEffortToSelectValue,
} from "./state.js";

function appendModelOptions(select) {
  const followOption = document.createElement("option");
  followOption.value = "";
  followOption.textContent = "跟随请求模型";
  select.appendChild(followOption);
  state.apiModelOptions.forEach((model) => {
    const option = document.createElement("option");
    option.value = model.slug;
    option.textContent = model.displayName || model.slug;
    select.appendChild(option);
  });
}

function appendReasoningOptions(select) {
  REASONING_OPTIONS.forEach((optionItem) => {
    const option = document.createElement("option");
    option.value = optionItem.value;
    option.textContent = optionItem.label;
    select.appendChild(option);
  });
}

function syncEffortState(modelSelect, effortSelect) {
  const hasModelOverride = Boolean((modelSelect.value || "").trim());
  effortSelect.disabled = !hasModelOverride;
  if (!hasModelOverride) {
    effortSelect.value = "";
  }
}

function createModelCell(item, onUpdateModel) {
  const cellModel = document.createElement("td");
  const modelWrap = document.createElement("div");
  modelWrap.className = "cell-stack";
  const modelSelect = document.createElement("select");
  modelSelect.className = "inline-select";
  appendModelOptions(modelSelect);

  const effortSelect = document.createElement("select");
  effortSelect.className = "inline-select";
  appendReasoningOptions(effortSelect);

  modelSelect.value = item.modelSlug || "";
  effortSelect.value = mapReasoningEffortToSelectValue(item.reasoningEffort);
  modelSelect.addEventListener("change", () => {
    syncEffortState(modelSelect, effortSelect);
    onUpdateModel?.(item, modelSelect.value, effortSelect.value);
  });
  effortSelect.addEventListener("change", () => {
    onUpdateModel?.(item, modelSelect.value, effortSelect.value);
  });
  syncEffortState(modelSelect, effortSelect);
  modelWrap.appendChild(modelSelect);
  modelWrap.appendChild(effortSelect);
  cellModel.appendChild(modelWrap);
  return cellModel;
}

function createStatusCell(item) {
  const cellStatus = document.createElement("td");
  const statusViewModel = getStatusViewModel(item.status);
  const statusTag = document.createElement("span");
  statusTag.className = "status-tag";
  statusTag.classList.add(statusViewModel.className);
  statusTag.textContent = statusViewModel.label;
  cellStatus.appendChild(statusTag);
  return { cellStatus, isDisabled: statusViewModel.isDisabled };
}

function createUsedCell(item) {
  const cellUsed = document.createElement("td");
  cellUsed.textContent = item.lastUsedAt
    ? new Date(item.lastUsedAt * 1000).toLocaleString()
    : "-";
  return cellUsed;
}

function createActionsCell(item, isDisabled, { onToggleStatus, onDelete }) {
  const cellActions = document.createElement("td");
  const actionsWrap = document.createElement("div");
  actionsWrap.className = "cell-actions";
  const btnDisable = document.createElement("button");
  btnDisable.className = "secondary";
  btnDisable.textContent = isDisabled ? "启用" : "禁用";
  btnDisable.addEventListener("click", () => onToggleStatus?.(item));

  const btnDelete = document.createElement("button");
  btnDelete.className = "danger";
  btnDelete.textContent = "删除";
  btnDelete.addEventListener("click", () => onDelete?.(item));
  actionsWrap.appendChild(btnDisable);
  actionsWrap.appendChild(btnDelete);
  cellActions.appendChild(actionsWrap);
  return cellActions;
}

function renderApiKeyRow(item, handlers) {
  const row = document.createElement("tr");
  const cellId = document.createElement("td");
  cellId.className = "mono";
  cellId.textContent = item.id;

  const cellName = document.createElement("td");
  cellName.textContent = item.name || "-";

  const cellProfile = document.createElement("td");
  const protocolType = item.protocolType || "openai_compat";
  cellProfile.textContent = getProtocolProfileLabel(protocolType);

  const cellModel = createModelCell(item, handlers.onUpdateModel);
  const { cellStatus, isDisabled } = createStatusCell(item);
  const cellUsed = createUsedCell(item);
  const cellActions = createActionsCell(item, isDisabled, handlers);

  row.appendChild(cellId);
  row.appendChild(cellName);
  row.appendChild(cellProfile);
  row.appendChild(cellModel);
  row.appendChild(cellStatus);
  row.appendChild(cellUsed);
  row.appendChild(cellActions);
  dom.apiKeyRows.appendChild(row);
}

function renderEmptyRow() {
  const emptyRow = document.createElement("tr");
  const emptyCell = document.createElement("td");
  emptyCell.colSpan = 7;
  emptyCell.textContent = "暂无平台 Key";
  emptyRow.appendChild(emptyCell);
  dom.apiKeyRows.appendChild(emptyRow);
}

// 渲染 API Key 列表
export function renderApiKeys({ onToggleStatus, onDelete, onUpdateModel }) {
  dom.apiKeyRows.innerHTML = "";
  if (state.apiKeyList.length === 0) {
    renderEmptyRow();
    return;
  }

  state.apiKeyList.forEach((item) => {
    renderApiKeyRow(item, {
      onToggleStatus,
      onDelete,
      onUpdateModel,
    });
  });
}
