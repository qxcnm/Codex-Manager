import { remainingPercent } from "../utils/format.js";

export function buildUsageRows(accountList, usageList) {
  return accountList.map((account) => {
    const usage = usageList.find((item) => item.accountId === account.id);
    return {
      accountLabel: account.label,
      accountSub: account.id,
      primaryRemain: remainingPercent(usage ? usage.usedPercent : null),
      secondaryRemain: remainingPercent(usage ? usage.secondaryUsedPercent : null),
      primaryResetsAt: usage ? usage.resetsAt : null,
      secondaryResetsAt: usage ? usage.secondaryResetsAt : null,
    };
  });
}

