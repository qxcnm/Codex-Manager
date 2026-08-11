export type AccountCallability =
  | "callable"
  | "disabled"
  | "confirmed_deactivated"
  | "reauthenticate"
  | "auth_failed"
  | "quota_limited"
  | "network_unknown"
  | "unprobed"
  | "unavailable";

export interface AccountCallabilityInput {
  status?: string | null;
  statusReason?: string | null;
  credentialState?: string | null;
  credentialAction?: string | null;
  gatewayProbeStatus?: string | null;
  gatewayProbeReason?: string | null;
}

function normalized(value: string | null | undefined): string {
  return String(value || "").trim().toLowerCase();
}

/**
 * Resolve the status users actually care about: whether the gateway may route
 * a request to this credential. Subscription/usage health alone is not proof
 * that the Responses endpoint accepts the current credential.
 */
export function resolveAccountCallability(
  input: AccountCallabilityInput,
): AccountCallability {
  const status = normalized(input.status);
  const statusReason = normalized(input.statusReason);
  const credentialState = normalized(input.credentialState);
  const credentialAction = normalized(input.credentialAction);
  const probeStatus = normalized(input.gatewayProbeStatus);
  const probeReason = normalized(input.gatewayProbeReason);

  if (
    status === "banned" ||
    credentialAction === "stop" ||
    ["account_deactivated", "workspace_deactivated"].includes(credentialState) ||
    ["account_deactivated", "workspace_deactivated", "deactivated_workspace"].includes(
      statusReason,
    )
  ) {
    return "confirmed_deactivated";
  }

  if (status === "disabled" || status === "inactive") {
    return "disabled";
  }

  if (
    [
      "refresh_token_expired",
      "refresh_token_revoked",
      "reauth_required",
      "reauth_in_progress",
    ].includes(credentialState) ||
    credentialAction === "reauthenticate"
  ) {
    return "reauthenticate";
  }

  if (
    ["access_token_expired", "access_token_rejected"].includes(credentialState) ||
    credentialAction === "refresh" ||
    [
      "codex_responses_unauthorized",
      "codex_unauthorized",
      "usage_status_401",
    ].includes(probeReason) ||
    statusReason === "usage_http_401"
  ) {
    return "auth_failed";
  }

  if (
    status === "limited" ||
    probeReason === "quota_exhausted" ||
    statusReason === "usage_limit_exhausted"
  ) {
    return "quota_limited";
  }

  if (
    credentialState === "network_unknown" ||
    credentialAction === "retry_network" ||
    statusReason === "usage_cloudflare_challenge" ||
    [
      "codex_responses_probe_failed",
      "codex_models_probe_failed",
      "cloudflare_challenge",
    ].includes(probeReason)
  ) {
    return "network_unknown";
  }

  if (
    status === "active" &&
    probeStatus === "available" &&
    probeReason === "codex_responses_verified"
  ) {
    return "callable";
  }

  if (!probeStatus || probeStatus === "unprobed" || probeStatus === "pending") {
    return "unprobed";
  }

  return "unavailable";
}

export function accountCallabilityText(callability: AccountCallability): string {
  switch (callability) {
    case "callable":
      return "可调用";
    case "disabled":
      return "已停用";
    case "confirmed_deactivated":
      return "已确认停用";
    case "reauthenticate":
      return "需要重新登录";
    case "auth_failed":
      return "调用授权失败";
    case "quota_limited":
      return "额度受限";
    case "network_unknown":
      return "网络状态待确认";
    case "unprobed":
      return "尚未探测";
    default:
      return "不可调用";
  }
}
