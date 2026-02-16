import { dom } from "../ui/dom";
import { state } from "../state";
import { copyText } from "../utils/clipboard.js";

function formatTs(ts) {
  if (!ts) return "-";
  const date = new Date(ts * 1000);
  if (Number.isNaN(date.getTime())) return "-";
  return date.toLocaleString();
}

export function renderRequestLogs() {
  dom.requestLogRows.innerHTML = "";
  const filtered = state.requestLogList.filter((item) => {
    const filter = state.requestLogStatusFilter || "all";
    if (filter === "all") return true;
    const code = Number(item.statusCode);
    if (!Number.isFinite(code)) return false;
    if (filter === "2xx") return code >= 200 && code < 300;
    if (filter === "4xx") return code >= 400 && code < 500;
    if (filter === "5xx") return code >= 500 && code < 600;
    return true;
  });
  if (!filtered.length) {
    const row = document.createElement("tr");
    const cell = document.createElement("td");
    cell.colSpan = 8;
    cell.textContent = "暂无请求日志";
    row.appendChild(cell);
    dom.requestLogRows.appendChild(row);
    return;
  }

  const fragment = document.createDocumentFragment();
  filtered.forEach((item) => {
    const row = document.createElement("tr");
    const cellTime = document.createElement("td");
    cellTime.textContent = formatTs(item.createdAt);
    row.appendChild(cellTime);

    const cellKey = document.createElement("td");
    cellKey.textContent = item.keyId || "-";
    row.appendChild(cellKey);

    const cellMethod = document.createElement("td");
    cellMethod.textContent = item.method || "-";
    row.appendChild(cellMethod);

    const cellPath = document.createElement("td");
    const pathWrap = document.createElement("div");
    pathWrap.className = "request-path-wrap";
    const pathText = document.createElement("span");
    pathText.className = "request-path";
    pathText.textContent = item.requestPath || "-";
    const copyBtn = document.createElement("button");
    copyBtn.className = "ghost path-copy";
    copyBtn.type = "button";
    copyBtn.textContent = "复制";
    copyBtn.title = "复制请求路径";
    copyBtn.addEventListener("click", async () => {
      if (!item.requestPath) return;
      const ok = await copyText(item.requestPath);
      if (ok) {
        copyBtn.textContent = "已复制";
      } else {
        copyBtn.textContent = "失败";
      }
      setTimeout(() => {
        copyBtn.textContent = "复制";
      }, 900);
    });
    pathWrap.appendChild(pathText);
    pathWrap.appendChild(copyBtn);
    cellPath.appendChild(pathWrap);
    row.appendChild(cellPath);

    const cellModel = document.createElement("td");
    cellModel.textContent = item.model || "-";
    row.appendChild(cellModel);

    const cellEffort = document.createElement("td");
    cellEffort.textContent = item.reasoningEffort || "-";
    row.appendChild(cellEffort);

    const cellStatus = document.createElement("td");
    const statusTag = document.createElement("span");
    statusTag.className = "status-tag";
    const code = item.statusCode == null ? null : Number(item.statusCode);
    statusTag.textContent = code == null ? "-" : String(code);
    if (code != null) {
      if (code >= 200 && code < 300) {
        statusTag.classList.add("status-ok");
      } else if (code >= 400 && code < 500) {
        statusTag.classList.add("status-warn");
      } else if (code >= 500) {
        statusTag.classList.add("status-bad");
      } else {
        statusTag.classList.add("status-unknown");
      }
    } else {
      statusTag.classList.add("status-unknown");
    }
    cellStatus.appendChild(statusTag);
    row.appendChild(cellStatus);

    const cellError = document.createElement("td");
    cellError.textContent = item.error || "-";
    row.appendChild(cellError);
    fragment.appendChild(row);
  });
  dom.requestLogRows.appendChild(fragment);
}
