import { state } from "../state";
import * as api from "../api";

let requestLogRefreshSeq = 0;

function ensureRpcSuccess(result, fallbackMessage) {
  if (result && typeof result === "object" && typeof result.error === "string" && result.error) {
    throw new Error(result.error);
  }
  if (result == null) {
    throw new Error(fallbackMessage);
  }
  return result;
}

// 刷新账号列表
export async function refreshAccounts() {
  const res = ensureRpcSuccess(await api.serviceAccountList(), "读取账号列表失败");
  state.accountList = Array.isArray(res.items) ? res.items : [];
}

// 刷新用量列表
export async function refreshUsageList() {
  const res = ensureRpcSuccess(await api.serviceUsageList(), "读取用量列表失败");
  state.usageList = Array.isArray(res.items) ? res.items : [];
}

// 刷新 API Key 列表
export async function refreshApiKeys() {
  const res = ensureRpcSuccess(await api.serviceApiKeyList(), "读取平台 Key 列表失败");
  state.apiKeyList = Array.isArray(res.items) ? res.items : [];
}

// 刷新模型下拉选项（来自平台上游 /v1/models）
export async function refreshApiModels() {
  const res = ensureRpcSuccess(await api.serviceApiKeyModels(), "读取模型列表失败");
  state.apiModelOptions = Array.isArray(res.items) ? res.items : [];
}

// 刷新请求日志（按关键字过滤）
export async function refreshRequestLogs(query, options = {}) {
  const latestOnly = options.latestOnly !== false;
  const seq = ++requestLogRefreshSeq;
  const res = ensureRpcSuccess(
    await api.serviceRequestLogList(query || null, 300),
    "读取请求日志失败",
  );
  if (latestOnly && seq !== requestLogRefreshSeq) {
    return false;
  }
  state.requestLogList = Array.isArray(res.items) ? res.items : [];
  return true;
}

export async function clearRequestLogs() {
  return ensureRpcSuccess(await api.serviceRequestLogClear(), "清空请求日志失败");
}
