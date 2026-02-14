import { state } from "../state.js";
import { dom } from "../ui/dom.js";
import { calcAvailability, formatTs, remainingPercent } from "../utils/format.js";
import { findUsage } from "./usage.js";

function normalizeGroupName(value) {
  return String(value || "").trim();
}

export function buildGroupFilterOptions(accounts) {
  const list = Array.isArray(accounts) ? accounts : [];
  const counter = new Map();
  for (const account of list) {
    const group = normalizeGroupName(account && account.groupName);
    if (!group) continue;
    counter.set(group, (counter.get(group) || 0) + 1);
  }

  const dynamicGroups = Array.from(counter.entries())
    .sort((left, right) => left[0].localeCompare(right[0], "zh-Hans-CN"))
    .map(([value, count]) => ({
      value,
      label: value,
      count,
    }));

  return [
    {
      value: "all",
      label: "全部分组",
      count: list.length,
    },
    ...dynamicGroups,
  ];
}

function syncGroupFilterSelect(options) {
  if (!dom.accountGroupFilter) return;
  const select = dom.accountGroupFilter;
  const safeOptions = Array.isArray(options) ? options : [];
  const nextValues = new Set(safeOptions.map((item) => item.value));

  // 中文注释：分组来自实时账号数据；若分组被删除/重命名，不自动回退会导致列表“看似空白”且用户难定位原因。
  if (!nextValues.has(state.accountGroupFilter)) {
    state.accountGroupFilter = "all";
  }

  select.innerHTML = "";
  for (const option of safeOptions) {
    const node = document.createElement("option");
    node.value = option.value;
    node.textContent = `${option.label} (${option.count})`;
    if (option.value === state.accountGroupFilter) {
      node.selected = true;
    }
    select.appendChild(node);
  }
  if (!nextValues.has(state.accountGroupFilter)) {
    select.value = "all";
  }
}

// 渲染账号列表
export function filterAccounts(accounts, usageList, query, filter, groupFilter = "all") {
  const keyword = String(query || "").trim().toLowerCase();
  const normalizedGroupFilter = normalizeGroupName(groupFilter) || "all";
  const usageMap = new Map((usageList || []).map((item) => [item.accountId, item]));

  return (accounts || []).filter((account) => {
    if (keyword) {
      const label = String(account.label || "").toLowerCase();
      const id = String(account.id || "").toLowerCase();
      if (!label.includes(keyword) && !id.includes(keyword)) return false;
    }

    if (normalizedGroupFilter !== "all") {
      const accountGroup = normalizeGroupName(account.groupName);
      if (accountGroup !== normalizedGroupFilter) return false;
    }

    if (filter === "active" || filter === "low") {
      const usage = usageMap.get(account.id);
      const primaryRemain = remainingPercent(usage ? usage.usedPercent : null);
      const secondaryRemain = remainingPercent(
        usage ? usage.secondaryUsedPercent : null,
      );
      const status = calcAvailability(usage);
      if (filter === "active" && status.level !== "ok") return false;
      if (
        filter === "low" &&
        !(
          (primaryRemain != null && primaryRemain <= 20) ||
          (secondaryRemain != null && secondaryRemain <= 20)
        )
      ) {
        return false;
      }
    }
    return true;
  });
}

export function renderAccounts({ onUpdateSort, onOpenUsage, onDelete }) {
  dom.accountRows.innerHTML = "";
  syncGroupFilterSelect(buildGroupFilterOptions(state.accountList));

  const filtered = filterAccounts(
    state.accountList,
    state.usageList,
    state.accountSearch,
    state.accountFilter,
    state.accountGroupFilter,
  );

  if (filtered.length === 0) {
    const emptyRow = document.createElement("tr");
    const emptyCell = document.createElement("td");
    emptyCell.colSpan = 6;
    emptyCell.textContent = state.accountList.length === 0 ? "暂无账号" : "当前筛选条件下无结果";
    emptyRow.appendChild(emptyCell);
    dom.accountRows.appendChild(emptyRow);
    return;
  }

  filtered.forEach((account) => {
    const row = document.createElement("tr");
    const usage = findUsage(account.id);
    const status = calcAvailability(usage);

    const cellAccount = document.createElement("td");
    const accountWrap = document.createElement("div");
    accountWrap.className = "cell-stack";
    const primaryRemain = remainingPercent(usage ? usage.usedPercent : null);
    const secondaryRemain = remainingPercent(
      usage ? usage.secondaryUsedPercent : null,
    );
    const accountTitle = document.createElement("strong");
    accountTitle.textContent = account.label || "-";
    const accountMeta = document.createElement("small");
    accountMeta.textContent = `${account.id || "-"}`;
    accountWrap.appendChild(accountTitle);
    accountWrap.appendChild(accountMeta);
    const mini = document.createElement("div");
    mini.className = "mini-usage";
    mini.appendChild(
      renderMiniUsageLine("5小时", primaryRemain, false),
    );
    mini.appendChild(
      renderMiniUsageLine("7天", secondaryRemain, true),
    );
    accountWrap.appendChild(mini);
    cellAccount.appendChild(accountWrap);

    const cellGroup = document.createElement("td");
    cellGroup.textContent = normalizeGroupName(account.groupName) || "-";

    const cellSort = document.createElement("td");
    const sortInput = document.createElement("input");
    sortInput.className = "sort-input";
    sortInput.type = "number";
    sortInput.value = account.sort != null ? String(account.sort) : "0";
    sortInput.addEventListener("change", async (event) => {
      const value = Number(event.target.value || 0);
      onUpdateSort?.(account.id, value);
    });
    cellSort.appendChild(sortInput);

    const cellStatus = document.createElement("td");
    const statusTag = document.createElement("span");
    statusTag.className = "status-tag";
    statusTag.textContent = status.text;
    if (status.level === "ok") statusTag.classList.add("status-ok");
    if (status.level === "warn") statusTag.classList.add("status-warn");
    if (status.level === "bad") statusTag.classList.add("status-bad");
    if (status.level === "unknown") statusTag.classList.add("status-unknown");
    cellStatus.appendChild(statusTag);

    const cellUpdated = document.createElement("td");
    const updatedText = document.createElement("strong");
    updatedText.textContent = usage && usage.capturedAt ? formatTs(usage.capturedAt) : "未知";
    cellUpdated.appendChild(updatedText);

    const cellActions = document.createElement("td");
    const actionsWrap = document.createElement("div");
    actionsWrap.className = "cell-actions";
    const btn = document.createElement("button");
    btn.className = "secondary";
    btn.textContent = "用量查询";
    btn.addEventListener("click", () => onOpenUsage?.(account));
    actionsWrap.appendChild(btn);

    const del = document.createElement("button");
    del.className = "danger";
    del.textContent = "删除";
    del.addEventListener("click", () => onDelete?.(account));
    actionsWrap.appendChild(del);
    cellActions.appendChild(actionsWrap);

    row.appendChild(cellAccount);
    row.appendChild(cellGroup);
    row.appendChild(cellSort);
    row.appendChild(cellStatus);
    row.appendChild(cellUpdated);
    row.appendChild(cellActions);
    dom.accountRows.appendChild(row);
  });
}

function renderMiniUsageLine(label, remain, secondary) {
  const line = document.createElement("div");
  line.className = "progress-line";
  if (secondary) line.classList.add("secondary");
  const text = document.createElement("span");
  text.textContent = `${label} ${remain == null ? "--" : `${remain}%`}`;
  const track = document.createElement("div");
  track.className = "track";
  const fill = document.createElement("div");
  fill.className = "fill";
  fill.style.width = remain == null ? "0%" : `${remain}%`;
  track.appendChild(fill);
  line.appendChild(text);
  line.appendChild(track);
  return line;
}

// 打开账号登录弹窗
export function openAccountModal() {
  dom.modalAccount.classList.add("active");
  dom.loginUrl.value = "";
  if (dom.manualCallbackUrl) {
    dom.manualCallbackUrl.value = "";
  }
  dom.loginHint.textContent = "点击登录后会打开浏览器完成授权。";
  dom.inputNote.value = "";
  dom.inputTags.value = "";
  dom.inputGroup.value = "TEAM";
}

// 关闭账号登录弹窗
export function closeAccountModal() {
  dom.modalAccount.classList.remove("active");
}

