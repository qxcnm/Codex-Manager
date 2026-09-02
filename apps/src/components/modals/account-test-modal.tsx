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

function isImageModel(model: ManagedModelV2): boolean {
  const caps = (model.capabilities ?? {}) as Record<string, unknown>;
  return (
    caps.supports_image_generation === true ||
    caps.supportsImageGeneration === true
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
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  return `test-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

export function AccountTestModal({
  account,
  open,
  onOpenChange,
  onFinished,
}: AccountTestModalProps) {
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
  const finishedRef = useRef(false);
  const unlistenRef = useRef<(() => void) | null>(null);
  const phaseRef = useRef<Phase>("idle");
  const accountIdRef = useRef<string | null>(account?.id ?? null);
  const onFinishedRef = useRef(onFinished);
  const terminalRef = useRef<HTMLDivElement>(null);

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
        setState((prev) => ({ ...prev, text: prev.text + (payload.text ?? "") }));
        break;
      case "image": {
        const imageUrl = payload.imageUrl;
        if (imageUrl) {
          setState((prev) => ({
            ...prev,
            images: [
              ...prev.images,
              { url: imageUrl, mimeType: payload.mimeType ?? "image/png" },
            ],
          }));
        }
        break;
      }
      case "status":
        setState((prev) => ({ ...prev, status: payload.status ?? prev.status }));
        break;
      case "test_complete":
        setState((prev) => ({ ...prev, success: payload.success ?? true }));
        setPhase("done");
        finishedRef.current = true;
        if (accountIdRef.current) {
          onFinishedRef.current?.(accountIdRef.current);
        }
        break;
      case "error":
        setState((prev) => ({
          ...prev,
          error: payload.error ?? "测试失败",
        }));
        setPhase("done");
        finishedRef.current = true;
        if (accountIdRef.current) {
          onFinishedRef.current?.(accountIdRef.current);
        }
        break;
    }
  }, []);

  const startTest = useCallback(async () => {
    const id = accountIdRef.current;
    if (!id || phaseRef.current === "running") {
      return;
    }
    unlistenRef.current?.();
    unlistenRef.current = null;
    // 订阅前先持有本次测试的 testId，事件到达时即可按 testId 隔离，避免并发测试串流。
    const testId = newTestId();
    testIdRef.current = testId;
    finishedRef.current = false;
    setState({ text: "", images: [] });
    setCanceled(false);
    setPhase("running");

    try {
      const unlisten = await listenAccountTestEvent(handleEvent);
      unlistenRef.current = unlisten;
      const result = await accountClient.testAccount({
        accountId: id,
        model: selectedModel ?? undefined,
        kind: testKind,
        testId,
      });
      setState((prev) => ({ ...prev, model: result.model ?? prev.model }));
    } catch (err) {
      unlistenRef.current?.();
      unlistenRef.current = null;
      setState((prev) => ({
        ...prev,
        error: err instanceof Error ? err.message : "启动测试失败",
      }));
      finishedRef.current = true;
      setPhase("done");
      if (accountIdRef.current) {
        onFinishedRef.current?.(accountIdRef.current);
      }
    }
  }, [handleEvent, selectedModel, testKind]);

  const cancelTest = useCallback(() => {
    const id = accountIdRef.current;
    if (id) {
      void accountClient.cancelAccountTest(id).catch(() => {});
    }
    unlistenRef.current?.();
    unlistenRef.current = null;
    testIdRef.current = null;
    finishedRef.current = true;
    setState({ text: "", images: [] });
    setCanceled(true);
    setPhase("idle");
  }, []);

  const handleOpenChange = useCallback(
    (nextOpen: boolean) => {
      if (!nextOpen && phaseRef.current === "running") {
        const id = accountIdRef.current;
        if (id) {
          void accountClient.cancelAccountTest(id).catch(() => {});
        }
      }
      onOpenChange(nextOpen);
    },
    [onOpenChange],
  );

  useEffect(() => {
    if (!open || !accountId) {
      return;
    }

    setPhase("idle");
    setState({ text: "", images: [] });
    setCanceled(false);
    testIdRef.current = null;
    finishedRef.current = false;
    unlistenRef.current?.();
    unlistenRef.current = null;
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
        setSelectedModel((textModel ?? enabled[0])?.slug ?? null);
      } catch {
        // 模型列表加载失败不阻塞测试，后端会用默认文字模型兜底。
      }
    })();

    return () => {
      disposed = true;
      unlistenRef.current?.();
      unlistenRef.current = null;
    };
  }, [open, accountId]);

  const { text, images, status, success, error } = state;

  // 按来源分组展示：官方内置模型与自定义模型分开，方便识别哪些是官方目录、哪些可增删。
  const builtinModels = models.filter((model) => model.origin === "builtin");
  const customModels = models.filter((model) => model.origin !== "builtin");

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="glass-card max-h-[calc(100vh-2rem)] overflow-hidden sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>测试账号</DialogTitle>
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
              测试类型
            </label>
            <Select value={testKind} onValueChange={handleTestKindChange}>
              <SelectTrigger
                id="account-test-kind"
                disabled={phase === "running"}
                className="w-full"
              >
                <SelectValue placeholder="选择测试类型">
                  {(value) => {
                    const v = value == null ? "" : String(value);
                    return v === "image"
                      ? "图片模型"
                      : v === "text"
                        ? "文字模型"
                        : "选择测试类型";
                  }}
                </SelectValue>
              </SelectTrigger>
              <SelectContent alignItemWithTrigger={false}>
                <SelectGroup>
                  <SelectItem value="text">文字模型</SelectItem>
                  <SelectItem value="image">图片模型</SelectItem>
                </SelectGroup>
              </SelectContent>
            </Select>
          </div>

          <div className="grid gap-1.5">
            <label htmlFor="account-test-model" className="text-xs text-muted-foreground">
              测试模型
            </label>
            <Select
              value={selectedModel}
              onValueChange={(value) => {
                const slug = value ? String(value) : null;
                setSelectedModel(slug);
                // 让测试类型跟随所选模型能力，避免「文字直连 + 图片专用模型」把
                // gpt-image-2 当主模型直连、被上游判为不支持。
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
                <SelectValue placeholder="选择模型">
                  {(value) => {
                    const valueStr = value == null ? "" : String(value);
                    const model = models.find((item) => item.slug === valueStr);
                    return model ? modelLabel(model) : "选择模型";
                  }}
                </SelectValue>
              </SelectTrigger>
              <SelectContent alignItemWithTrigger={false}>
                {builtinModels.length > 0 ? (
                  <SelectGroup>
                    <SelectLabel>官方模型</SelectLabel>
                    {builtinModels.map((model) => (
                      <SelectItem key={model.slug} value={model.slug}>
                        {modelLabel(model)}
                        {isImageModel(model) ? "（图片）" : ""}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                ) : null}
                {customModels.length > 0 ? (
                  <SelectGroup>
                    <SelectLabel>自定义模型</SelectLabel>
                    {customModels.map((model) => (
                      <SelectItem key={model.slug} value={model.slug}>
                        {modelLabel(model)}
                        {isImageModel(model) ? "（图片）" : ""}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                ) : null}
              </SelectContent>
            </Select>
            {models.length === 0 ? (
              <span className="text-xs text-muted-foreground">
                未加载到可用模型，测试将使用后端默认模型。
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
                    ? "已取消测试，可再次点击「开始测试」。"
                    : "准备就绪，点击「开始测试」发起一次真实请求。"}
                </span>
              </div>
            ) : (
              <>
                {state.model ? (
                  <div className="mb-1 text-gray-500">模型：{state.model}</div>
                ) : null}
                {status ? (
                  <div className="mb-2 flex items-center gap-2 text-yellow-400">
                    {phase === "running" ? (
                      <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin" />
                    ) : null}
                    <span>{status}</span>
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
                      <span>{success ? "测试成功" : error || "测试失败"}</span>
                    </div>
                    {success && isManuallyDisabled(account) ? (
                      <div className="mt-1 text-xs text-amber-400">
                        该账号为手动禁用，测试虽成功但不会被自动恢复为「可用」。
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
                图片预览
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
              取消
            </Button>
          ) : null}
          {phase === "done" ? (
            <Button type="button" onClick={startTest}>
              重试
            </Button>
          ) : null}
          {phase === "idle" ? (
            <Button type="button" onClick={startTest}>
              开始测试
            </Button>
          ) : null}
          <Button
            type="button"
            variant="ghost"
            onClick={() => handleOpenChange(false)}
          >
            关闭
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
