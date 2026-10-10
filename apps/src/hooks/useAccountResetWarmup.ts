"use client";

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { accountClient } from "@/lib/api/account-client";
import { buildAccountListQueryKey } from "@/lib/api/account-query-keys";
import { getAppErrorMessage } from "@/lib/api/transport";
import { useI18n } from "@/lib/i18n/provider";

export function useAccountResetWarmup(serviceAddr: string, isServiceReady: boolean) {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const mutation = useMutation({
    mutationFn: ({ accountIds, enabled }: { accountIds: string[]; enabled: boolean }) =>
      accountClient.updateResetWarmup(accountIds, enabled),
    onSuccess: async (result, variables) => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: buildAccountListQueryKey(serviceAddr) }),
        queryClient.invalidateQueries({ queryKey: ["startup-snapshot"] }),
      ]);
      toast.success(
        variables.enabled
          ? t("已为 {count} 个账号开启额度重置自动唤醒", { count: result.updated })
          : t("已为 {count} 个账号关闭额度重置自动唤醒", { count: result.updated }),
      );
    },
    onError: (error: unknown) => {
      toast.error(t("更新额度重置自动唤醒失败: {error}", { error: getAppErrorMessage(error) }));
    },
  });

  return {
    isUpdatingResetWarmup: mutation.isPending,
    setAccountResetWarmupEnabled: (accountIds: string[], enabled: boolean) => {
      if (!isServiceReady || mutation.isPending) return;
      const normalizedIds = [...new Set(accountIds.map((id) => id.trim()).filter(Boolean))];
      if (normalizedIds.length === 0) return;
      mutation.mutate({ accountIds: normalizedIds, enabled });
    },
  };
}
