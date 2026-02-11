import { dom } from "../ui/dom";
import { remainingPercent } from "../utils/format";

export function renderRecommendations(accounts, usageMap) {
  if (!dom.recommendations) return;
  dom.recommendations.innerHTML = "";
  const header = document.createElement("div");
  header.className = "panel-header";
  const title = document.createElement("h3");
  title.textContent = "最佳账号推荐";
  const hint = document.createElement("span");
  hint.className = "hint";
  hint.textContent = "按剩余额度";
  header.appendChild(title);
  header.appendChild(hint);
  dom.recommendations.appendChild(header);

  if (!accounts.length) {
    const empty = document.createElement("div");
    empty.className = "hint";
    empty.textContent = "暂无可推荐账号";
    dom.recommendations.appendChild(empty);
    return;
  }

  const list = document.createElement("div");
  list.className = "mini-usage";

  const primaryPick = pickBest(accounts, usageMap, false);
  const secondaryPick = pickBest(accounts, usageMap, true);
  list.appendChild(
    renderRecommendationItem("用于 5小时", primaryPick?.account, primaryPick?.remain),
  );
  list.appendChild(
    renderRecommendationItem("用于 7天", secondaryPick?.account, secondaryPick?.remain),
  );

  dom.recommendations.appendChild(list);
}

function pickBest(accounts, usageMap, secondary) {
  const ranked = accounts
    .map((account) => {
      const usage = usageMap.get(account.id);
      const remain = remainingPercent(
        usage ? (secondary ? usage.secondaryUsedPercent : usage.usedPercent) : null,
      );
      return { account, remain };
    })
    .filter((item) => item.remain != null)
    .sort((a, b) => (b.remain ?? 0) - (a.remain ?? 0));
  return ranked[0] || null;
}

function renderRecommendationItem(label, account, remain) {
  const item = document.createElement("div");
  item.className = "cell";
  const itemLabel = document.createElement("small");
  itemLabel.textContent = label;
  item.appendChild(itemLabel);
  if (!account) {
    const empty = document.createElement("strong");
    empty.textContent = "暂无账号";
    item.appendChild(empty);
    return item;
  }
  const accountLabel = document.createElement("strong");
  accountLabel.textContent = account.label || "-";
  const accountId = document.createElement("small");
  accountId.textContent = account.id || "-";
  item.appendChild(accountLabel);
  item.appendChild(accountId);
  const badge = document.createElement("span");
  badge.className = "status-tag status-ok";
  badge.textContent = remain == null ? "--" : `${remain}%`;
  item.appendChild(badge);
  return item;
}
