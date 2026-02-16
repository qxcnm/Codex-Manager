import { calcAvailability, remainingPercent } from "../../utils/format.js";

export function normalizeGroupName(value) {
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
