"use client";

import { useEffect, useRef, useState } from "react";
import {
  Cable,
  ChevronLeft,
  ChevronRight,
  KeyRound,
  ShieldCheck,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { useI18n } from "@/lib/i18n/provider";
import { cn } from "@/lib/utils";
import { buildStaticRouteUrl } from "@/lib/utils/static-routes";

interface CodexCliOnboardingDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onAcknowledge: (dismissPermanently: boolean) => Promise<void>;
}

const GUIDE_STEPS = [
  {
    icon: ShieldCheck,
    title: "第一步：确认服务已连接",
    description: "先确认 CodexManager 本地服务可用，再选择 Codex 接入模式。",
    details: [
      "顶部或设置页显示“服务已连接”。",
      "默认网关地址是 `http://localhost:48760/v1`。",
    ],
  },
  {
    icon: KeyRound,
    title: "第二步：准备账号或平台密钥",
    description:
      "账号直连需要 active OpenAI 账号；本地网关需要可用的平台密钥。",
    details: ["去添加 OpenAI 账号", "去创建平台密钥"],
  },
  {
    icon: Cable,
    title: "第三步：在平台模式应用配置",
    description:
      "选择模式与目标后点击应用，页面会调用现有 profile 接口写入配置。",
    details: [
      "选择账号直连或本地网关后，CodexManager 会接管该 Codex profile 的 auth.json / config.toml。",
      "无需复制配置模板，也不要把账号 token 手动写进 auth.json。",
    ],
  },
] as const;

export function CodexCliOnboardingDialog({
  open,
  onOpenChange,
  onAcknowledge,
}: CodexCliOnboardingDialogProps) {
  const { t } = useI18n();
  const [currentStep, setCurrentStep] = useState(0);
  const [dismissPermanently, setDismissPermanently] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const introFocusRef = useRef<HTMLDivElement | null>(null);
  const scrollContainerRef = useRef<HTMLDivElement | null>(null);
  const activeStep = GUIDE_STEPS[currentStep];
  const ActiveStepIcon = activeStep.icon;
  const isFirstStep = currentStep === 0;
  const isLastStep = currentStep === GUIDE_STEPS.length - 1;

  useEffect(() => {
    if (!open) return;
    setCurrentStep(0);
    const resetScroll = () => {
      scrollContainerRef.current?.scrollTo({
        top: 0,
        left: 0,
        behavior: "auto",
      });
    };
    resetScroll();
    const rafId = window.requestAnimationFrame(resetScroll);
    return () => window.cancelAnimationFrame(rafId);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    scrollContainerRef.current?.scrollTo({ top: 0, left: 0, behavior: "auto" });
  }, [currentStep, open]);

  const handleAcknowledge = async () => {
    setIsSaving(true);
    try {
      await onAcknowledge(dismissPermanently);
      setDismissPermanently(false);
    } finally {
      setIsSaving(false);
    }
  };

  const handleOpenPlatformMode = async () => {
    setIsSaving(true);
    try {
      await onAcknowledge(dismissPermanently);
      window.location.assign(buildStaticRouteUrl("/platform-mode"));
    } finally {
      setIsSaving(false);
    }
  };

  const handleSessionClose = () => {
    setDismissPermanently(false);
    onOpenChange(false);
  };

  const handleRequestClose = () => {
    if (dismissPermanently) {
      void handleAcknowledge();
      return;
    }
    handleSessionClose();
  };

  const handleOpenChange = (nextOpen: boolean) => {
    if (isSaving) return;
    if (!nextOpen) {
      handleRequestClose();
      return;
    }
    onOpenChange(nextOpen);
  };

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent
        initialFocus={introFocusRef}
        className="glass-card mission-panel overflow-hidden p-0 sm:!max-w-[min(92vw,920px)]"
        style={{ height: "76vh", maxHeight: "700px" }}
      >
        <div
          className="grid h-full min-h-0"
          style={{ gridTemplateRows: "auto minmax(0, 1fr) auto" }}
        >
          <DialogHeader className="shrink-0 border-b border-border/60 px-5 py-4">
            <div
              ref={introFocusRef}
              tabIndex={-1}
              className="max-w-3xl select-none space-y-1 outline-none"
            >
              <DialogTitle className="text-lg font-semibold md:text-xl">
                {t("Codex 首次接入引导")}
              </DialogTitle>
              <DialogDescription className="text-xs leading-5 md:text-sm">
                {t(
                  "无需手动编辑 auth.json 或 config.toml。CodexManager 会通过平台模式安全写入并备份 Codex profile。",
                )}
              </DialogDescription>
            </div>
          </DialogHeader>

          <div
            ref={scrollContainerRef}
            data-testid="codex-guide-scroll"
            className="grid min-h-0 content-start gap-4 overflow-y-auto overscroll-contain px-5 py-4 lg:grid-cols-[minmax(0,1fr)_minmax(320px,0.82fr)]"
            style={{ scrollbarGutter: "stable", WebkitOverflowScrolling: "touch" }}
          >
            <section className="min-w-0 rounded-md border border-border/60 bg-background/45 p-4 shadow-sm">
              <div className="space-y-1 border-b border-border/50 pb-3">
                <h3 className="text-base font-semibold leading-7 text-foreground">
                  {t("基础步骤")}
                </h3>
                <p className="text-sm leading-6 text-muted-foreground">
                  {t("你当前在第 {current} 步，共 {total} 步。", {
                    current: currentStep + 1,
                    total: GUIDE_STEPS.length,
                  })}
                </p>
              </div>

              <div className="mt-3 grid gap-2 md:grid-cols-3 lg:grid-cols-1">
                {GUIDE_STEPS.map((step, index) => {
                  const StepIcon = step.icon;
                  return (
                    <Button
                      key={step.title}
                      type="button"
                      variant="outline"
                      onClick={() => setCurrentStep(index)}
                      className={cn(
                        "h-auto min-w-0 items-start justify-start gap-3 rounded-md px-3 py-3 text-left whitespace-normal",
                        index === currentStep
                          ? "border-primary/30 bg-primary/10 text-foreground shadow-sm"
                          : "border-border/60 bg-background/70 text-muted-foreground hover:bg-accent/50",
                      )}
                    >
                      <StepIcon className="mt-0.5 h-4 w-4 shrink-0" />
                      <span className="min-w-0 text-sm leading-5 font-medium">
                        {t(step.title)}
                      </span>
                    </Button>
                  );
                })}
              </div>

              <div className="mt-3 rounded-md border border-border/60 bg-background/70 p-4">
                <div className="flex items-start gap-4">
                  <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-md border border-primary/15 bg-primary/10 text-primary">
                    <ActiveStepIcon className="h-5 w-5" />
                  </div>
                  <div className="min-w-0 space-y-2">
                    <h4 className="text-base font-semibold leading-7 text-foreground">
                      {t(activeStep.title)}
                    </h4>
                    <p className="text-sm leading-6 text-muted-foreground">
                      {t(activeStep.description)}
                    </p>
                    <ul className="list-disc space-y-1 pl-5 text-sm leading-6 text-muted-foreground">
                      {activeStep.details.map((detail) => (
                        <li key={detail}>{t(detail)}</li>
                      ))}
                    </ul>
                  </div>
                </div>
              </div>
            </section>

            <section className="min-w-0 rounded-md border border-border/60 bg-background/55 p-4 shadow-sm">
              <div className="space-y-1 border-b border-border/50 pb-3">
                <h3 className="text-base font-semibold leading-7 text-foreground">
                  {t("平台模式选择")}
                </h3>
                <p className="text-sm leading-6 text-muted-foreground">
                  {t(
                    "请统一使用平台模式，避免 provider、模型目录和运行时重载配置彼此不一致。",
                  )}
                </p>
              </div>

              <div className="mt-3 space-y-3">
                <div className="rounded-md border border-border/60 bg-background/70 p-3">
                  <div className="font-medium text-foreground">{t("账号直连")}</div>
                  <p className="mt-1 text-sm leading-6 text-muted-foreground">
                    {t(
                      "直连 OpenAI 官方后端，不经过 CodexManager 网关；不会产生 CodexManager 请求日志，仪表盘用量统计不可用。",
                    )}
                  </p>
                </div>
                <div className="rounded-md border border-primary/20 bg-primary/5 p-3">
                  <div className="font-medium text-foreground">{t("本地网关")}</div>
                  <p className="mt-1 text-sm leading-6 text-muted-foreground">
                    {t(
                      "通过 CodexManager 本地网关转发 Codex CLI 请求；请求日志、Token、费用估算和仪表盘统计可用。",
                    )}
                  </p>
                </div>
                <Button
                  type="button"
                  className="w-full gap-2"
                  onClick={() => void handleOpenPlatformMode()}
                  disabled={isSaving}
                >
                  <Cable className="h-4 w-4" />
                  {isSaving ? t("保存中...") : t("打开平台模式")}
                </Button>
              </div>
            </section>
          </div>

          <DialogFooter className="mx-0 mb-0 shrink-0 rounded-b-lg border-t border-border/60 bg-background/95 px-5 py-3 sm:flex-nowrap sm:items-center sm:justify-between">
            <label className="flex items-center gap-3 pr-4 text-sm text-muted-foreground">
              <Checkbox
                checked={dismissPermanently}
                onCheckedChange={(checked) =>
                  setDismissPermanently(Boolean(checked))
                }
                disabled={isSaving}
                aria-label={t("下次不再显示这份引导")}
              />
              <span className="leading-6">{t("下次不再显示这份引导")}</span>
            </label>
            <div className="flex shrink-0 flex-col-reverse gap-2 sm:flex-row">
              {!isFirstStep ? (
                <Button
                  type="button"
                  variant="outline"
                  className="gap-2"
                  onClick={() => setCurrentStep((step) => Math.max(0, step - 1))}
                  disabled={isSaving}
                >
                  <ChevronLeft className="h-4 w-4" />
                  {t("上一步")}
                </Button>
              ) : null}
              {!isLastStep ? (
                <Button
                  type="button"
                  variant="outline"
                  className="gap-2"
                  onClick={() =>
                    setCurrentStep((step) =>
                      Math.min(GUIDE_STEPS.length - 1, step + 1),
                    )
                  }
                  disabled={isSaving}
                >
                  {t("下一步")}
                  <ChevronRight className="h-4 w-4" />
                </Button>
              ) : null}
              {!dismissPermanently ? (
                <Button
                  type="button"
                  variant="outline"
                  onClick={handleSessionClose}
                  disabled={isSaving}
                >
                  {t("本次关闭")}
                </Button>
              ) : null}
              {isLastStep || dismissPermanently ? (
                <Button
                  type="button"
                  onClick={() => void handleAcknowledge()}
                  disabled={isSaving}
                >
                  {isSaving
                    ? t("保存中...")
                    : dismissPermanently
                      ? t("保存并关闭")
                      : t("我已阅读")}
                </Button>
              ) : null}
            </div>
          </DialogFooter>
        </div>
      </DialogContent>
    </Dialog>
  );
}
