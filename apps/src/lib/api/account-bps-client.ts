import { invoke, withAddr } from "./transport";

export interface BpsLoginStart {
  loginId: string;
  userCode: string;
  verificationUri: string;
  interval: number;
  expiresAt: number;
}

export interface BpsLoginStatus {
  status: "pending" | "completed" | "failed";
  error?: string;
}

export const accountBpsClient = {
  update(accountIds: string[], enabled: boolean, addr: string | null) {
    return invoke<{ updated: number }>(
      "service_account_bps_update",
      withAddr({ accountIds, enabled, addr }),
      { retries: 0 },
    );
  },
  startLogin(accountId: string, addr: string | null) {
    return invoke<BpsLoginStart>(
      "service_account_bps_login_start",
      withAddr({ accountId, addr }),
      { retries: 0 },
    );
  },
  loginStatus(accountId: string, loginId: string, addr: string | null) {
    return invoke<BpsLoginStatus>(
      "service_account_bps_login_status",
      withAddr({ accountId, loginId, addr }),
      { retries: 0 },
    );
  },
};
