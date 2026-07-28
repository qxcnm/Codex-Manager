"use client";

import { useEffect } from "react";
import { reportClientError } from "@/lib/client-logger";
import { DEFAULT_LOCALE, normalizeLocale } from "@/lib/i18n/config";
import { translate } from "@/lib/i18n/messages";

export default function GlobalError({
  error,
  reset,
}: {
  error: Error & { digest?: string };
  reset: () => void;
}) {
  const locale =
    typeof navigator === "undefined"
      ? DEFAULT_LOCALE
      : normalizeLocale(navigator.language);
  const t = (message: string) => translate(locale, message);

  useEffect(() => {
    reportClientError("global_error_boundary", error);
  }, [error]);

  return (
    <html lang="zh-CN">
      <body>
        <main
          style={{
            alignItems: "center",
            display: "flex",
            justifyContent: "center",
            minHeight: "100vh",
            padding: 24,
          }}
        >
          <section style={{ maxWidth: 480, textAlign: "center" }}>
            <h1>{t("应用发生异常")}</h1>
            <p>{t("异常信息已写入应用日志，请重试。")}</p>
            <button type="button" onClick={reset}>
              {t("重试")}
            </button>
          </section>
        </main>
      </body>
    </html>
  );
}
