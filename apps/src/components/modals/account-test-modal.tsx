"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { CheckCircle2, Loader2, XCircle } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { accountClient } from "@/lib/api/account-client";
import { managedModelsV2Client } from "@/lib/api/managed-models-v2";
import {
  listenAccountTestEvent,
  type AccountTestEventPayload,
} from "@/lib/api/account-test-events";
import { AccountStatusCell } from "@/app/accounts/accounts-page-helpers";
import { useI18n } from "@/lib/i18n/provider";
import type { ManagedModelV2 } from "@/types/model-v2";
import type { Account } from "@/types";

interface AccountTestModalProps {
  account: Account | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onFinished?: (accountId: string) => void;
}

interface TestImage {
  url: string;
  mimeType: string;
}

interface Accumulated {
  text: string;
  images: TestImage[];
  model?: string;
  status?: string;
  success?: boolean;
  error?: string;
}

type Phase = "idle" | "running" | "done";
const DEFAULT_ACCOUNT_TEST_MODEL = "gpt-6-luna";
const MAX_TEST_OUTPUT_CHARS = 200_000;
const MAX_TEST_IMAGES = 4;

function isImageModel(model: ManagedModelV2): boolean {
  const caps = (model.capabilities ?? {}) as Record<string, unknown>;
  const outputModalities = [caps.output_modalities, caps.outputModalities].flatMap(
    (value) => (Array.isArray(value) ? value : []),
  );
  return (
    caps.supports_image_generation === true ||
    caps.supportsImageGeneration === true ||
    outputModalities.some(
      (modality) =>
        typeof modality === "string" && modality.trim().toLowerCase() === "image",
    )
  );
}

function isManuallyDisabled(account: Account | null): boolean {
  const status = String(account?.status ?? "").trim().toLowerCase();
  return status === "disabled" || status === "inactive";
}

function modelLabel(model: ManagedModelV2): string {
  const name = model.displayName?.trim();
  return name || model.slug;
}

function newTestId(): string {
  const cryptoApi = globalThis.crypto;
  if (typeof cryptoApi?.randomUUID === "function") {
    return cryptoApi.randomUUID();
  }
  if (typeof cryptoApi?.getRandomValues !== "function") {
    throw new Error("Secure random values are unavailable");
  }
  const bytes = cryptoApi.getRandomValues(new Uint8Array(16));
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0"));
  return [
    hex.slice(0, 4).join(""),
    hex.slice(4, 6).join(""),
    hex.slice(6, 8).join(""),
    hex.slice(8, 10).join(""),
    hex.slice(10).join(""),
  ].join("-");
}

export function AccountTestModal({
  account,
  open,
  onOpenChange,
  onFinished,
}: AccountTestModalProps) {
  const { t } = useI18n();
  const [phase, setPhase] = useState<Phase>("idle");
  const [state, setState] = useState<Accumulated>({ text: "", images: [] });
  const [models, setModels] = useState<ManagedModelV2[]>([]);
  const [selectedModel, setSelectedModel] = useState<string | null>(null);
  const [testKind, setTestKind] = useState<"text" | "image">("text");
  const [canceled, setCanceled] = useState(false);

  // 测试类型只决定发哪种请求（文字直连 / 图片工具），不干预模型列表的选择。
  const handleTestKindChange = (value: string | null) => {
    const nextKind = value === "image" ? "image" : "text";
    setTestKind(nextKind);
    // 类型切换后保持模型一致：当前模型不符合新类型时，自动选一个匹配的模型。
    const current = models.find((item) => item.slug === selectedModel);
    if (!current || isImageModel(current) !== (nextKind === "image")) {
      const match = models.find(
        (item) => isImageModel(item) === (nextKind === "image"),
      );
      setSelectedModel(match?.slug ?? null);
    }
  };

  const testIdRef = useRef<string | null>(null);
  const runTokenRef = useRef(0);
  const finishedRef = useRef(false);
  const unlistenRef = useRef<(() => void) | null>(null);
  const listenerAbortRef = useRef<AbortController | null>(null);
  const phaseRef = useRef<Phase>("idle");
  const accountIdRef = useRef<string | null>(account?.id ?? null);
  const onFinishedRef = useRef(onFinished);
  const terminalRef = useRef<HTMLDivElement>(null);
  const pendingTextChunksRef = useRef<string[]>([]);
  const pendingTextChunkOffsetRef = useRef(0);
  const pendingTextLengthRef = useRef(0);
  const textFlushFrameRef = useRef<number | null>(null);

  const accountId = account?.id ?? null;
  accountIdRef.current = accountId;
  onFinishedRef.current = onFinished;

  useEffect(() => {
    phaseRef.current = phase;
  }, [phase]);

  useEffect(() => {
    const el = terminalRef.current;
    if (el) {
      el.scrollTop = el.scrollHeight;
    }
  }, [state.text, state.status, phase, state.images.length]);

  const appendPendingText = useCallback((chunk: string) => {
    const chunks = pendingTextChunksRef.current;
    chunks.push(chunk);
    pendingTextLengthRef.current += chunk.length;

    let offset = pendingTextChunkOffsetRef.current;
    while (pendingTextLengthRef.current > MAX_TEST_OUTPUT_CHARS && offset < chunks.length) {
      const overflow = pendingTextLengthRef.current - MAX_TEST_OUTPUT_CHARS;
      const currentChunk = chunks[offset];
      if (currentChunk.length <= overflow) {
        pendingTextLengthRef.current -= currentChunk.length;
        offset += 1;
      } else {
        chunks[offset] = currentChunk.slice(overflow);
        pendingTextLengthRef.current -= overflow;
        break;
      }
    }

    if (offset >= chunks.length) {
      chunks.length = 0;
      offset = 0;
    } else if (offset > 64 && offset * 2 >= chunks.length) {
      chunks.splice(0, offset);
      offset = 0;
    }
    pendingTextChunkOffsetRef.current = offset;
  }, []);

  const flushPendingText = useCallback(() => {
    const nextText = pendingTextChunksRef.current
      .slice(pendingTextChunkOffsetRef.current)
      .join("");
    setState((prev) =>
      prev.text === nextText ? prev : { ...prev, text: nextText },
    );
  }, []);

  const scheduleTextFlush = useCallback(() => {
    if (textFlushFrameRef.current !== null) return;
    textFlushFrameRef.current = window.requestAnimationFrame(() => {
      textFlushFrameRef.current = null;
      flushPendingText();
    });
  }, [flushPendingText]);

  const resetOutputBuffer = useCallback(() => {
    pendingTextChunksRef.current.length = 0;
    pendingTextChunkOffsetRef.current = 0;
    pendingTextLengthRef.current = 0;
    if (textFlushFrameRef.current !== null) {
      window.cancelAnimationFrame(textFlushFrameRef.current);
      textFlushFrameRef.current = null;
    }
  }, []);

  const stopEventListener = useCallback(() => {
    const unlisten = unlistenRef.current;
    unlistenRef.current = null;
    unlisten?.();
    const controller = listenerAbortRef.current;
    listenerAbortRef.current = null;
    controller?.abort();
  }, []);

  const handleEvent = useCallback((payload: AccountTestEventPayload) => {
    const currentId = testIdRef.current;
    if (currentId && payload.testId && payload.testId !== currentId) {
      return;
    }
    if (finishedRef.current) {
      return;
    }
    switch (payload.type) {
      case "test_start":
        setState((prev) => ({ ...prev, model: payload.model ?? prev.model }));
        break;
      case "content":
        if (typeof payload.text === "string" && payload.text) {
          appendPendingText(payload.text);
          scheduleTextFlush();
        }
        break;
      case "image": {
        const imageUrl = payload.imageUrl;
        if (imageUrl) {
          setState((prev) => {
            if (prev.images.length >= MAX_TEST_IMAGES) return prev;
            return {
              ...prev,
              images: [
                ...prev.images,
                { url: imageUrl, mimeType: payload.mimeType ?? "image/png" },
              ],
            };
          });
        }
        break;
      }
      case "status":
        setState((prev) => ({ ...prev, status: payload.status ?? prev.status }));
        break;
      case "test_complete":
        flushPendingText();
        finishedRef.current = true;
        phaseRef.current = "done";
        stopEventListener();
        testIdRef.current = null;
        setState((prev) => ({ ...prev, success: payload.success ?? true }));
        setPhase("done");
        if (accountIdRef.current) {
          onFinishedRef.current?.(accountIdRef.current);
        }
        break;
      case "error":
        flushPendingText();
        finishedRef.current = true;
        phaseRef.current = "done";
        stopEventListener();
        testIdRef.current = null;
        setState((prev) => ({
          ...prev,
          error: payload.error ?? t("测试失败"),
        }));
        setPhase("done");
        if (accountIdRef.current) {
          onFinishedRef.current?.(accountIdRef.current);
        }
        break;
    }
  }, [appendPendingText, flushPendingText, scheduleTextFlush, stopEventListener, t]);

  const startTest = useCallback(async () => {
    const id = accountIdRef.current;
    if (!id || phaseRef.current === "running") {
      return;
    }
    const runToken = ++runTokenRef.current;
    stopEventListener();
    const listenerAbortController = new AbortController();
    listenerAbortRef.current = listenerAbortController;
    // 订阅前先持有本次测试的 testId，事件到达时即可按 testId 隔离，避免并发测试串流。
    let testId: string;
    try {
      testId = newTestId();
    } catch {
      setState({ text: "", images: [], error: t("启动测试失败") });
      setCanceled(false);
      finishedRef.current = true;
      phaseRef.current = "done";
      setPhase("done");
      stopEventListener();
      return;
    }

    if (
      runToken !== runTokenRef.current ||
      !open ||
      accountIdRef.current !== id ||
      listenerAbortController.signal.aborted
    ) {
      void accountClient.cancelAccountTest(id, testId).catch(() => {});
      listenerAbortController.abort();
      if (listenerAbortRef.current === listenerAbortController) {
        listenerAbortRef.current = null;
      }
      return;
    }

    testIdRef.current = testId;
    finishedRef.current = false;
    resetOutputBuffer();
    setState({ text: "", images: [] });
    setCanceled(false);
    phaseRef.current = "running";
    setPhase("running");

    try {
      const unlisten = await listenAccountTestEvent(testId, handleEvent, {
        signal: listenerAbortController.signal,
      });
      if (
        runToken !== runTokenRef.current ||
        !open ||
        accountIdRef.current !== id
      ) {
        unlisten();
        if (listenerAbortRef.current === listenerAbortController) {
          listenerAbortRef.current = null;
        }
        return;
      }
      unlistenRef.current = unlisten;
      const result = await accountClient.testAccount({
        accountId: id,
        model: selectedModel ?? undefined,
        kind: testKind,
        testId,
      });
      if (runToken !== runTokenRef.current) {
        void accountClient.cancelAccountTest(id, testId).catch(() => {});
        return;
      }
      setState((prev) => ({ ...prev, model: result.model ?? prev.model }));
    } catch (err) {
      if (runToken !== runTokenRef.current) {
        void accountClient.cancelAccountTest(id, testId).catch(() => {});
        return;
      }
      stopEventListener();
      setState((prev) => ({
        ...prev,
        error:
          err instanceof Error && err.message.trim()
            ? err.message
            : t("启动测试失败"),
      }));
      finishedRef.current = true;
      phaseRef.current = "done";
      setPhase("done");
      if (accountIdRef.current) {
        onFinishedRef.current?.(accountIdRef.current);
      }
    }
  }, [handleEvent, open, resetOutputBuffer, selectedModel, stopEventListener, t, testKind]);

  const cancelTest = useCallback(() => {
    runTokenRef.current += 1;
    const id = accountIdRef.current;
    const testId = testIdRef.current;
    if (id && testId) {
      void accountClient.cancelAccountTest(id, testId).catch(() => {});
    }
    stopEventListener();
    testIdRef.current = null;
    finishedRef.current = true;
    resetOutputBuffer();
    setState({ text: "", images: [] });
    setCanceled(true);
    phaseRef.current = "idle";
    setPhase("idle");
  }, [resetOutputBuffer, stopEventListener]);

  const handleOpenChange = useCallback(
    (nextOpen: boolean) => {
      if (!nextOpen && phaseRef.current === "running") {
        cancelTest();
      }
      onOpenChange(nextOpen);
    },
    [cancelTest, onOpenChange],
  );

  useEffect(() => {
    if (!open || !accountId) {
      return;
    }

    runTokenRef.current += 1;
    phaseRef.current = "idle";
    setPhase("idle");
    resetOutputBuffer();
    setState({ text: "", images: [] });
    setCanceled(false);
    testIdRef.current = null;
    finishedRef.current = false;
    stopEventListener();
    setModels([]);
    setSelectedModel(null);
    setTestKind("text");

    let disposed = false;
    (async () => {
      try {
        const result = await managedModelsV2Client.list(true);
        if (disposed) {
          return;
        }
        const enabled = result.items.filter((model) => model.enabled);
        setModels(enabled);
        const textModel = enabled.find((model) => !isImageModel(model));
        const preferredTextModel = enabled.find(
          (model) =>
            model.slug.toLowerCase() === DEFAULT_ACCOUNT_TEST_MODEL &&
            !isImageModel(model),
        );
        setSelectedModel(
          (preferredTextModel ?? textModel ?? enabled[0])?.slug ?? null,
        );
      } catch {
        // 模型列表加载失败不阻塞测试，后端会用默认文字模型兜底。
      }
    })();

    return () => {
      disposed = true;
      runTokenRef.current += 1;
      const activeTestId = testIdRef.current;
      if (accountId && activeTestId && phaseRef.current === "running") {
        void accountClient.cancelAccountTest(accountId, activeTestId).catch(() => {});
      }
      stopEventListener();
      if (textFlushFrameRef.current !== null) {
        window.cancelAnimationFrame(textFlushFrameRef.current);
        textFlushFrameRef.current = null;
      }
      resetOutputBuffer();
    };
  }, [accountId, open, resetOutputBuffer, stopEventListener]);

  const { text, images, status, success, error } = state;

  // 按来源分组展示：官方内置模型与自定义模型分开，方便识别哪些是官方目录、哪些可增删。
  const builtinModels = models.filter((model) => model.origin === "builtin");
  const customModels = models.filter((model) => model.origin !== "builtin");

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="glass-card max-h-[calc(100vh-2rem)] overflow-hidden sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("测试账号")}</DialogTitle>
          <DialogDescription>
            {account?.name || account?.label || accountId}
          </DialogDescription>
        </DialogHeader>

        <div className="grid max-h-[calc(100vh-16rem)] gap-4 overflow-y-auto px-6 py-2">
          {account ? (
            <div className="rounded-lg border border-border/70 bg-card/40 px-3 py-2">
              <AccountStatusCell account={account} />
            </div>
          ) : null}

          <div className="grid gap-1.5">
            <label htmlFor="account-test-kind" className="text-xs text-muted-foreground">
              {t("测试类型")}
            </label>
            <Select value={testKind} onValueChange={handleTestKindChange}>
              <SelectTrigger
                id="account-test-kind"
                disabled={phase === "running"}
                className="w-full"
              >
                <SelectValue placeholder={t("选择测试类型")}>
                  {(value) => {
                    const v = value == null ? "" : String(value);
                    return v === "image"
                      ? t("图片模型")
                      : v === "text"
                        ? t("文字模型")
                        : t("选择测试类型");
                  }}
                </SelectValue>
              </SelectTrigger>
              <SelectContent alignItemWithTrigger={false}>
                <SelectGroup>
                  <SelectItem value="text">{t("文字模型")}</SelectItem>
                  <SelectItem value="image">{t("图片模型")}</SelectItem>
                </SelectGroup>
              </SelectContent>
            </Select>
          </div>

          <div className="grid gap-1.5">
            <label htmlFor="account-test-model" className="text-xs text-muted-foreground">
              {t("测试模型")}
            </label>
            <Select
              value={selectedModel}
              onValueChange={(value) => {
                const slug = value ? String(value) : null;
                setSelectedModel(slug);
                // 让测试类型跟随所选模型能力，避免将图片专用模型作为文字主模型直连。
                const model = models.find((item) => item.slug === slug);
                if (model) {
                  setTestKind(isImageModel(model) ? "image" : "text");
                }
              }}
            >
              <SelectTrigger
                id="account-test-model"
                disabled={phase === "running"}
                className="w-full"
              >
                <SelectValue placeholder={t("选择模型")}>
                  {(value) => {
                    const valueStr = value == null ? "" : String(value);
                    const model = models.find((item) => item.slug === valueStr);
                    return model ? modelLabel(model) : t("选择模型");
                  }}
                </SelectValue>
              </SelectTrigger>
              <SelectContent alignItemWithTrigger={false}>
                {builtinModels.length > 0 ? (
                  <SelectGroup>
                    <SelectLabel>{t("官方模型")}</SelectLabel>
                    {builtinModels.map((model) => (
                      <SelectItem key={model.slug} value={model.slug}>
                        {modelLabel(model)}
                        {isImageModel(model) ? ` (${t("图片")})` : ""}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                ) : null}
                {customModels.length > 0 ? (
                  <SelectGroup>
                    <SelectLabel>{t("自定义模型")}</SelectLabel>
                    {customModels.map((model) => (
                      <SelectItem key={model.slug} value={model.slug}>
                        {modelLabel(model)}
                        {isImageModel(model) ? ` (${t("图片")})` : ""}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                ) : null}
              </SelectContent>
            </Select>
            {models.length === 0 ? (
              <span className="text-xs text-muted-foreground">
                {t("未加载到可用模型，测试将使用后端默认模型。")}
              </span>
            ) : null}
          </div>

          <div
            ref={terminalRef}
            className="h-56 overflow-y-auto rounded-xl border border-gray-700 bg-gray-900 p-4 font-mono text-sm leading-5 dark:border-gray-800 dark:bg-black"
          >
            {phase === "idle" ? (
              <div className="flex items-center gap-2 text-gray-500">
                <span>
                  {canceled
                    ? t("已取消测试，可再次点击「开始测试」。")
                    : t("准备就绪，点击「开始测试」发起一次真实请求。")}
                </span>
              </div>
            ) : (
              <>
                {state.model ? (
                  <div className="mb-1 text-gray-500">
                    {t("模型：")}{state.model}
                  </div>
                ) : null}
                {status ? (
                  <div className="mb-2 flex items-center gap-2 text-yellow-400">
                    {phase === "running" ? (
                      <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin" />
                    ) : null}
                    <span>{t(status)}</span>
                  </div>
                ) : null}
                {text ? (
                  <div className="whitespace-pre-wrap break-words text-green-400">
                    {text}
                    {phase === "running" ? (
                      <span className="animate-pulse">_</span>
                    ) : null}
                  </div>
                ) : null}
                {phase === "done" ? (
                  <>
                    <div
                      className={`mt-3 flex items-center gap-2 border-t border-gray-700 pt-3 ${
                        success ? "text-green-400" : "text-red-400"
                      }`}
                    >
                      {success ? (
                        <CheckCircle2 className="h-4 w-4 shrink-0" />
                      ) : (
                        <XCircle className="h-4 w-4 shrink-0" />
                      )}
                      <span>{success ? t("测试成功") : error || t("测试失败")}</span>
                    </div>
                    {success && isManuallyDisabled(account) ? (
                      <div className="mt-1 text-xs text-amber-400">
                        {t("该账号为手动禁用，测试虽成功但不会被自动恢复为「可用」。")}
                      </div>
                    ) : null}
                  </>
                ) : null}
              </>
            )}
          </div>

          {images.length > 0 ? (
            <div className="grid gap-2">
              <span className="text-xs font-medium text-muted-foreground">
                {t("图片预览")}
              </span>
              <div className="grid grid-cols-2 gap-2">
                {images.map((image, index) => (
                  // eslint-disable-next-line @next/next/no-img-element
                  <img
                    key={`${index}-${image.mimeType}`}
                    src={image.url}
                    alt={`test-result-${index + 1}`}
                    className="max-h-64 w-full rounded-lg border border-border/70 object-contain"
                  />
                ))}
              </div>
            </div>
          ) : null}
        </div>

        <DialogFooter className="px-6 pb-6">
          {phase === "running" ? (
            <Button type="button" variant="outline" onClick={cancelTest}>
              {t("取消")}
            </Button>
          ) : null}
          {phase === "done" ? (
            <Button type="button" onClick={startTest}>
              {t("重试")}
            </Button>
          ) : null}
          {phase === "idle" ? (
            <Button type="button" onClick={startTest}>
              {t("开始测试")}
            </Button>
          ) : null}
          <Button
            type="button"
            variant="ghost"
            onClick={() => handleOpenChange(false)}
          >
            {t("关闭")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
