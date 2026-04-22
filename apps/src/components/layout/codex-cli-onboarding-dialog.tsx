"use client";

import { useEffect, useRef, useState } from "react";
import {
  ChevronLeft,
  ChevronRight,
  Copy,
  FileCog,
  Link2,
  Rocket,
} from "lucide-react";
import { toast } from "sonner";
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
import { copyTextToClipboard } from "@/lib/utils/clipboard";

interface CodexCliOnboardingDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onAcknowledge: (dismissPermanently: boolean) => Promise<void>;
}

const GUIDE_STEPS = [
  {
    icon: FileCog,
    title: "第一步：先确认 CodexManager 服务已经可用",
    description:
      "先确认软件本身已经连上本地服务，再去配 CLI。这样能避免你配置写对了，但实际上连的是错端口或者空服务。",
    details: [
      "打开软件后先看顶部或设置页里的“服务已连接”状态。",
      "如果你改过监听端口，后面的 `base_url` 也必须用同一个端口。",
      "服务没启动、证书异常或端口不一致时，CLI 配置不会生效。",
    ],
  },
  {
    icon: Rocket,
    title: "第二步：把下面这份配置写入 Codex CLI 配置文件",
    description:
      "推荐先复制这份配置模板，再按你的实际端口或运行习惯微调。不要手敲 provider 名称，最容易在这里拼错。",
    details: [
      "通常放在 `~/.codex/config.toml`。",
      "在 Windows 上一般是 `%USERPROFILE%\\\\.codex\\\\config.toml`。",
      "如果你已经有现成配置，建议先备份，再把这段内容合并进去。",
    ],
  },
  {
    icon: Link2,
    title: "第三步：保存后重新启动 Codex CLI 并验证 provider",
    description:
      "最后只检查两件关键事：provider 名称一致，和 `base_url` 指向本软件的本地网关。只要这两项错一个，CLI 就不会走 CodexManager。",
    details: [
      '`model_provider = "cm"` 必须和 `[model_providers.cm]` 完全一致。',
      "`base_url` 默认应指向 `http://localhost:48760/v1`。",
      "如果你在 Web 端部署并访问，可以去模型管理页点击“导出到本地 Codex 缓存”；浏览器会下载同名 `models_cache.json`，你再手动放入本地 `~/.codex/models_cache.json`。",
      "如果你在设置里换过端口，把这里同步改掉后再重新打开 CLI 测试。",
    ],
  },
] as const;

const GUIDE_CONFIG_LINES = [
  {
    comment: "主对话模型，推荐直接使用 gpt-5.4 作为默认工作模型",
    line: 'model = "gpt-5.4"',
  },
  {
    comment: "默认模型提供方，填写 cm 代表走下面定义的本地 provider",
    line: 'model_provider = "cm"',
  },
  {
    comment: "代码审查或 review 场景使用的模型，这里也保持与主模型一致",
    line: 'review_model = "gpt-5.4"',
  },
  {
    comment: "人格预设，none 代表不额外附加风格化人格",
    line: 'personality = "none"',
  },
  {
    comment: "普通执行任务时的推理强度，xhigh 适合复杂代码与分析任务",
    line: 'model_reasoning_effort = "xhigh"',
  },
  {
    comment: "进入 plan mode 时的推理强度，继续保持 xhigh 便于做完整拆解",
    line: 'plan_mode_reasoning_effort = "xhigh"',
  },
  {
    comment: "是否输出推理摘要，detailed 表示尽量返回更详细的摘要信息",
    line: 'model_reasoning_summary = "detailed"',
  },
  {
    comment: "输出详略程度，medium 兼顾信息量与可读性",
    line: 'model_verbosity = "medium"',
  },
  {
    comment: "声明当前模型支持 reasoning summary，避免 CLI 错误判断能力",
    line: "model_supports_reasoning_summaries = true",
  },
  {
    comment: "需要用户审批时采用按需询问模式，危险操作会先确认",
    line: 'approval_policy = "on-request"',
  },
  {
    comment: "允许 login shell，方便某些环境变量和 shell 初始化脚本生效",
    line: "allow_login_shell = true",
  },
  {
    comment: "沙箱模式使用 workspace-write，只允许在工作区内读写",
    line: 'sandbox_mode = "workspace-write"',
  },
  {
    comment: "CLI 认证信息存储方式，file 表示保存在本地文件",
    line: 'cli_auth_credentials_store = "file"',
  },
  {
    comment: "ChatGPT 后端接口地址，保持官方默认地址即可",
    line: 'chatgpt_base_url = "https://chatgpt.com/backend-api/"',
  },
  {
    comment: "MCP OAuth 凭据存储方式，auto 表示交给 CLI 自动选择",
    line: 'mcp_oauth_credentials_store = "auto"',
  },
  {
    comment: "启动时自动检查更新，方便跟进新版本",
    line: "check_for_update_on_startup = true",
  },
  {
    comment: "Web 搜索模式，live 代表允许实时联网搜索",
    line: 'web_search = "live"',
  },
  {
    comment: "审批的审核方，这里设为当前用户本人",
    line: 'approvals_reviewer = "user"',
  },
  {
    comment: "服务层级，fast 通常能兼顾延迟和可用性",
    line: 'service_tier = "fast"',
  },
  {
    comment: null,
    line: "",
  },
  {
    comment:
      "定义名为 cm 的模型提供方，这个名字必须和上面的 model_provider 保持一致",
    line: "[model_providers.cm]",
  },
  {
    comment: "这个 provider 下的审批策略，继续沿用 on-request",
    line: 'approval_policy = "on-request"',
  },
  {
    comment: "这个 provider 下的沙箱策略，继续使用 workspace-write",
    line: 'sandbox_mode = "workspace-write"',
  },
  {
    comment: "这个 provider 下是否允许联网搜索，live 表示开启",
    line: 'web_search = "live"',
  },
  {
    comment: "展示名称，可写成 OpenAI 方便识别",
    line: 'name = "OpenAI"',
  },
  {
    comment: "本地网关地址，默认走 CodexManager 暴露出来的 48760 端口",
    line: 'base_url = "http://localhost:48760/v1"',
  },
  {
    comment: "与本软件网关对接时使用 responses 协议",
    line: 'wire_api = "responses"',
  },
] as const;

const GUIDE_REMINDERS = [
  "如果你在设置页改过服务端口，记得同步修改 `base_url`，否则 CLI 会连到旧端口。",
  "如果你在 Web 端想手动替换本地 Codex 缓存，优先用模型管理页右上角的导出按钮；它会下载同名 `models_cache.json` 供你手动放入本地 `.codex` 目录。",
  "如果 CLI 已经有其它 `model_providers` 配置，不需要全删，只要保证 `cm` 这一段完整且名字一致即可。",
  "只有勾选“下次不再显示这份引导”并点击确认后，软件才会把这个状态写入数据库；否则下次进入仍会再次提醒。",
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
  const codeBlockRef = useRef<HTMLPreElement | null>(null);
  const activeStep = GUIDE_STEPS[currentStep];
  const isFirstStep = currentStep === 0;
  const isLastStep = currentStep === GUIDE_STEPS.length - 1;
  const guideConfig = GUIDE_CONFIG_LINES.map(({ comment, line }) => {
    if (!line) {
      return "";
    }
    if (!comment) {
      return line;
    }
    return `# ${t(comment)}\n${line}`;
  }).join("\n");

  useEffect(() => {
    if (!open) {
      return;
    }

    setCurrentStep(0);
    const resetScroll = () => {
      scrollContainerRef.current?.scrollTo({
        top: 0,
        left: 0,
        behavior: "auto",
      });
      codeBlockRef.current?.scrollTo({ top: 0, left: 0, behavior: "auto" });
    };

    resetScroll();
    const rafId = window.requestAnimationFrame(resetScroll);
    return () => window.cancelAnimationFrame(rafId);
  }, [open]);

  useEffect(() => {
    if (!open) {
      return;
    }
    scrollContainerRef.current?.scrollTo({ top: 0, left: 0, behavior: "auto" });
  }, [currentStep, open]);

  const handleOpenChange = (nextOpen: boolean) => {
    if (isSaving) {
      return;
    }
    if (!nextOpen) {
      setDismissPermanently(false);
    }
    onOpenChange(nextOpen);
  };

  const handleAcknowledge = async () => {
    setIsSaving(true);
    try {
      await onAcknowledge(dismissPermanently);
      setDismissPermanently(false);
    } finally {
      setIsSaving(false);
    }
  };

  const handleCopyConfig = async () => {
    try {
      await copyTextToClipboard(guideConfig);
      toast.success(t("配置模板已复制"));
    } catch (error) {
      toast.error(error instanceof Error ? error.message : String(error));
    }
  };

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent
        initialFocus={introFocusRef}
        className="glass-card max-h-[92vh] overflow-hidden border-none p-0 sm:!max-w-[92vw] xl:!max-w-6xl"
      >
        <div className="flex min-h-0 max-h-[92vh] flex-col">
          <DialogHeader className="shrink-0 border-b border-border/60 px-6 pb-4 pt-6">
            <div className="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
              <div
                ref={introFocusRef}
                tabIndex={-1}
                className="max-w-3xl space-y-2 outline-none"
              >
                <DialogTitle className="text-2xl">
                  {t("Codex CLI 首次接入引导")}
                </DialogTitle>
                <DialogDescription className="text-sm leading-7">
                  {t(
                    "先看步骤说明，再复制配置模板去写 `config.toml`。只要没有勾选“不再显示”，你下次进入软件时仍会看到它。",
                  )}
                </DialogDescription>
              </div>
              <div className="rounded-2xl border border-primary/20 bg-primary/5 px-4 py-3 text-sm leading-6 text-muted-foreground lg:max-w-sm">
                {t(
                  "推荐先完整读一遍，再复制模板；这比自己手写 provider 名称和地址更不容易出错。",
                )}
              </div>
            </div>
          </DialogHeader>

          <div
            ref={scrollContainerRef}
            className="grid min-h-0 gap-5 overflow-y-auto px-6 py-5 lg:grid-cols-[minmax(0,1.15fr)_minmax(320px,0.85fr)]"
          >
            <div className="space-y-5">
              <section className="rounded-2xl border border-border/60 bg-background/45 p-5 shadow-sm">
                <div className="flex flex-col gap-4 border-b border-border/50 pb-4">
                  <div className="space-y-1">
                    <h3 className="text-base font-semibold leading-7 text-foreground">
                      {t("分步导引")}
                    </h3>
                    <p className="text-sm leading-6 text-muted-foreground">
                      {t("你当前在第 {current} 步，共 {total} 步。", {
                        current: currentStep + 1,
                        total: GUIDE_STEPS.length,
                      })}
                    </p>
                    <p className="text-xs leading-5 text-muted-foreground">
                      {t("点击步骤标签可直接跳转，按顺序做不容易漏。")}
                    </p>
                  </div>
                  <div className="grid gap-2 sm:grid-cols-3">
                    {GUIDE_STEPS.map((step, index) => (
                      <button
                        key={step.title}
                        type="button"
                        onClick={() => setCurrentStep(index)}
                        className={`rounded-2xl border px-3 py-3 text-left transition-colors ${
                          index === currentStep
                            ? "border-primary/40 bg-primary/10 text-foreground shadow-sm"
                            : "border-border/60 bg-background/70 text-muted-foreground hover:bg-accent/50"
                        }`}
                      >
                        <div className="text-xs font-semibold">
                          {t("步骤 {step}", { step: index + 1 })}
                        </div>
                        <div className="mt-1 line-clamp-2 text-sm font-medium leading-6">
                          {t(step.title)}
                        </div>
                      </button>
                    ))}
                  </div>
                </div>

                <div className="mt-4">
                  <section className="rounded-2xl border border-border/60 bg-background/70 p-4">
                    <div className="flex items-start gap-4">
                      <div className="flex h-11 w-11 shrink-0 items-center justify-center rounded-2xl bg-primary/10 text-primary">
                        <activeStep.icon className="h-5 w-5" />
                      </div>
                      <div className="min-w-0 space-y-2">
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="rounded-full bg-primary/10 px-2.5 py-1 text-xs font-semibold text-primary">
                            {t("步骤 {step}", { step: currentStep + 1 })}
                          </span>
                          <h4 className="text-base font-semibold leading-7 text-foreground">
                            {t(activeStep.title)}
                          </h4>
                        </div>
                        <p className="text-sm leading-7 text-muted-foreground">
                          {t(activeStep.description)}
                        </p>
                        <ul className="list-disc space-y-1.5 pl-5 text-sm leading-6 text-muted-foreground">
                          {activeStep.details.map((detail) => (
                            <li key={detail}>{t(detail)}</li>
                          ))}
                        </ul>
                      </div>
                    </div>
                  </section>
                </div>
              </section>

              <section className="rounded-2xl border border-dashed border-border/70 bg-muted/25 p-5">
                <h3 className="mb-2 text-base font-semibold leading-7 text-foreground">
                  {t("使用时最容易忽略的 3 个点")}
                </h3>
                <ul className="list-disc space-y-2 pl-5 text-sm leading-7 text-muted-foreground">
                  {GUIDE_REMINDERS.map((item) => (
                    <li key={item}>{t(item)}</li>
                  ))}
                </ul>
              </section>
            </div>

            <section className="rounded-2xl border border-border/60 bg-background/55 shadow-sm">
              <div className="flex flex-col gap-3 border-b border-border/60 px-5 py-4 sm:flex-row sm:items-start sm:justify-between">
                <div className="space-y-1">
                  <h3 className="text-base font-semibold leading-7 text-foreground">
                    {t("推荐配置示例")}
                  </h3>
                  <p className="text-sm leading-6 text-muted-foreground">
                    {t(
                      "已为每一行补充中文注释，可以直接复制后再按你的环境微调。",
                    )}
                  </p>
                </div>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="gap-2 self-start"
                  onClick={() => void handleCopyConfig()}
                >
                  <Copy className="h-4 w-4" />
                  {t("复制配置")}
                </Button>
              </div>
              <div className="p-5">
                <pre
                  ref={codeBlockRef}
                  className="max-h-[46vh] overflow-auto rounded-2xl border border-border/60 bg-black/90 p-4 font-mono text-xs leading-6 text-slate-100"
                >
                  <code>{guideConfig}</code>
                </pre>
              </div>
            </section>
          </div>

          <DialogFooter className="mx-0 mb-0 mt-auto rounded-b-xl border-t border-border/60 bg-background/90 px-6 py-4 shadow-[0_-8px_24px_rgba(15,23,42,0.06)] supports-backdrop-filter:backdrop-blur-sm sm:flex-nowrap sm:items-center sm:justify-between">
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
              {!isLastStep ? (
                <>
                  {!isFirstStep ? (
                    <Button
                      type="button"
                      variant="outline"
                      className="gap-2"
                      onClick={() =>
                        setCurrentStep((step) => Math.max(0, step - 1))
                      }
                      disabled={isSaving}
                    >
                      <ChevronLeft className="h-4 w-4" />
                      {t("上一步")}
                    </Button>
                  ) : null}
                  <Button
                    type="button"
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
                </>
              ) : null}
              <Button
                type="button"
                variant="outline"
                onClick={() => handleOpenChange(false)}
                disabled={isSaving}
              >
                {t("本次关闭")}
              </Button>
              {isLastStep ? (
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
