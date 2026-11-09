"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { accountBpsClient, type BpsLoginStart } from "@/lib/api/account-bps-client";
import { buildAccountListQueryKey, normalizeQueryServiceAddress } from "@/lib/api/account-query-keys";
import { getAppErrorMessage } from "@/lib/api/transport";
import { isBpsAuthorizationUrl, resolveBpsBatch } from "@/lib/account-bps";
import { useI18n } from "@/lib/i18n/provider";
import type { Account } from "@/types";

interface LoginSelection {
  accountId: string;
  accountName: string;
  addr: string | null;
  enableAfterLogin: boolean;
  login: BpsLoginStart | null;
  stage: "ready" | "starting" | "pending" | "completed" | "failed";
  error: string | null;
}

interface EnableConfirmation {
  accountIds: string[];
  addr: string | null;
  needsAuthorization: boolean;
}

export function useAccountBps({ accounts, serviceAddr, available }: {
  accounts: Account[];
  serviceAddr: string;
  available: boolean;
}) {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const addr = normalizeQueryServiceAddress(serviceAddr);
  const [selection, setSelection] = useState<LoginSelection | null>(null);
  const [confirmation, setConfirmation] = useState<EnableConfirmation | null>(null);
  const generation = useRef(0);
  const refreshAccounts = useCallback(async (requestAddr: string | null) => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: buildAccountListQueryKey(requestAddr) }),
      queryClient.invalidateQueries({ queryKey: ["startup-snapshot", requestAddr] }),
    ]);
  }, [queryClient]);
  const { mutateAsync: update, isPending: isUpdating } = useMutation({
    mutationFn: ({ accountIds, enabled, requestAddr }: {
      accountIds: string[];
      enabled: boolean;
      requestAddr: string | null;
    }) => accountBpsClient.update(accountIds, enabled, requestAddr),
    onSuccess: async (result, variables) => {
      await refreshAccounts(variables.requestAddr);
      toast.success(variables.enabled
        ? t("已为 {count} 个账号开启 BPS 转发", { count: result.updated })
        : t("已为 {count} 个账号关闭 BPS 转发", { count: result.updated }));
    },
    onError: (error: unknown) => {
      toast.error(t("更新 BPS 转发失败：{error}", { error: getAppErrorMessage(error) }));
    },
  });

  useEffect(() => {
    if (!selection || (available && selection.addr === addr)) return;
    generation.current += 1;
    const frame = window.requestAnimationFrame(() => setSelection(null));
    return () => window.cancelAnimationFrame(frame);
  }, [addr, available, selection]);

  useEffect(() => {
    if (!confirmation || (available && confirmation.addr === addr)) return;
    const frame = window.requestAnimationFrame(() => setConfirmation(null));
    return () => window.cancelAnimationFrame(frame);
  }, [addr, available, confirmation]);

  useEffect(() => {
    if (!available || selection?.addr !== addr || selection.stage !== "pending" || !selection.login) return;
    const { accountId, login, enableAfterLogin, addr: requestAddr } = selection;
    const selectedGeneration = generation.current;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    const intervalMs = Math.max(1, Math.min(login.interval || 5, 300)) * 1000;
    const fail = (message: string) => setSelection((current) => current?.login?.loginId === login.loginId
      ? { ...current, stage: "failed", error: message }
      : current);
    const poll = async () => {
      if (stopped || selectedGeneration !== generation.current) return;
      if (Date.now() >= login.expiresAt * 1000) {
        fail(t("BPS 授权已超时，请重新开始。"));
        return;
      }
      try {
        const result = await accountBpsClient.loginStatus(accountId, login.loginId, requestAddr);
        if (stopped || selectedGeneration !== generation.current) return;
        if (result.status === "failed") {
          fail(result.error || t("BPS 授权失败，请重试。"));
          return;
        }
        if (result.status === "completed") {
          // Keep this effect alive until the optional enable mutation finishes.
          await refreshAccounts(requestAddr);
          if (stopped || selectedGeneration !== generation.current) return;
          if (enableAfterLogin) {
            await update({ accountIds: [accountId], enabled: true, requestAddr });
          }
          if (stopped || selectedGeneration !== generation.current) return;
          setSelection((current) => current?.login?.loginId === login.loginId
            ? { ...current, stage: "completed", error: null }
            : current);
          if (!enableAfterLogin) toast.success(t("BPS 授权已保存"));
          return;
        }
        if (result.status !== "pending") {
          fail(t("BPS 授权响应无效，请重试。"));
          return;
        }
        timer = setTimeout(() => { void poll(); }, intervalMs);
      } catch (error) {
        if (!stopped && selectedGeneration === generation.current) fail(getAppErrorMessage(error));
      }
    };
    timer = setTimeout(() => { void poll(); }, intervalMs);
    return () => { stopped = true; clearTimeout(timer); };
  }, [addr, available, refreshAccounts, selection, t, update]);

  const beginLogin = (account: Account, enableAfterLogin: boolean) => {
    if (!available || isUpdating) return;
    generation.current += 1;
    setSelection({
      accountId: account.id, accountName: account.name, addr, enableAfterLogin,
      login: null, stage: "ready", error: null,
    });
  };
  const startLogin = async () => {
    if (!available || !selection || selection.addr !== addr || selection.stage === "starting" || selection.stage === "pending") return;
    const selectedGeneration = ++generation.current;
    const current = selection;
    setSelection({ ...current, login: null, stage: "starting", error: null });
    try {
      const login = await accountBpsClient.startLogin(current.accountId, current.addr);
      if (selectedGeneration !== generation.current) return;
      if (!login.loginId || !login.userCode || !isBpsAuthorizationUrl(login.verificationUri)
        || !Number.isFinite(login.expiresAt) || login.expiresAt * 1000 <= Date.now()) {
        throw new Error(t("BPS 授权响应无效，请重试。"));
      }
      setSelection({ ...current, login, stage: "pending", error: null });
    } catch (error) {
      if (selectedGeneration === generation.current) {
        setSelection({ ...current, login: null, stage: "failed", error: getAppErrorMessage(error) });
      }
    }
  };
  const setEnabled = async (accountIds: string[], enabled: boolean) => {
    if (!available || isUpdating) return;
    const batch = resolveBpsBatch(accounts, accountIds, enabled);
    if (batch.accountIds.length === 0) return;
    if (batch.missingIds.length > 0) {
      toast.error(t("账号列表已变化，请刷新后重试。"));
      return;
    }
    if (batch.unauthorizedIds.length > 0) {
      toast.error(t("选中账号中有 {count} 个尚未完成有效的 BPS 授权，请先逐个授权；本次未开启任何账号。", { count: batch.unauthorizedIds.length }));
      return;
    }
    if (enabled) {
      setConfirmation({ accountIds: batch.accountIds, addr, needsAuthorization: false });
      return;
    }
    try {
      await update({ accountIds: batch.accountIds, enabled, requestAddr: addr });
    } catch { /* The mutation reports the error without changing the switch. */ }
  };

  const confirmEnable = async () => {
    if (!available || isUpdating || !confirmation || confirmation.addr !== addr) return;
    const confirmed = confirmation;
    setConfirmation(null);
    if (confirmed.needsAuthorization && confirmed.accountIds.length === 1) {
      const account = accounts.find((item) => item.id === confirmed.accountIds[0]);
      if (!account) {
        toast.error(t("账号列表已变化，请刷新后重试。"));
        return;
      }
      beginLogin(account, true);
      return;
    }
    // The confirmation belongs to this exact selection and service address.
    const batch = resolveBpsBatch(accounts, confirmed.accountIds, true);
    if (batch.missingIds.length > 0 || batch.unauthorizedIds.length > 0) {
      toast.error(t("账号授权状态已变化，请刷新后重试。"));
      return;
    }
    try {
      await update({ accountIds: batch.accountIds, enabled: true, requestAddr: confirmed.addr });
    } catch { /* The mutation reports failures without changing saved state. */ }
  };

  return {
    isUpdating,
    available,
    selection: available && selection?.addr === addr ? selection : null,
    confirmation: available && confirmation?.addr === addr ? confirmation : null,
    confirmEnable,
    cancelConfirmation: () => setConfirmation(null),
    openLogin: (account: Account) => beginLogin(account, false),
    startLogin,
    setEnabled,
    toggle: (account: Account, enabled: boolean) => {
      if (!available || isUpdating) return;
      if (enabled && !account.bpsAuthorized) {
        setConfirmation({ accountIds: [account.id], addr, needsAuthorization: true });
      }
      else void setEnabled([account.id], enabled);
    },
    closeLogin: () => { generation.current += 1; setSelection(null); },
  };
}

export type AccountBpsController = ReturnType<typeof useAccountBps>;
