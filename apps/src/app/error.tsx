"use client";

import { useEffect } from "react";
import { reportClientError } from "@/lib/client-logger";
import { useI18n } from "@/lib/i18n/provider";

export default function AppError({
  error,
  reset,
}: {
  error: Error & { digest?: string };
  reset: () => void;
}) {
  const { t } = useI18n();

  useEffect(() => {
    reportClientError("app_error_boundary", error);
  }, [error]);

  return (
    <main className="flex min-h-screen items-center justify-center p-6">
      <section className="glass-card max-w-lg space-y-4 rounded-xl p-6 text-center">
        <h1 className="text-lg font-semibold">{t("页面发生异常")}</h1>
        <p className="text-sm text-muted-foreground">
          {t("异常信息已写入应用日志。你可以重试当前页面。")}
        </p>
        <button
          type="button"
          className="rounded-md border px-4 py-2 text-sm"
          onClick={reset}
        >
          {t("重试")}
        </button>
      </section>
    </main>
  );
}
