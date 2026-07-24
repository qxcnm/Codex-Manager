"use client";

import { useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button, buttonVariants } from "@/components/ui/button";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { getAppErrorMessage } from "@/lib/api/transport";
import { useI18n } from "@/lib/i18n/provider";
import type {
  ManagedModelImportConflictStrategyV2,
  ManagedModelImportPreviewV2Result,
  ManagedModelImportV2Params,
} from "@/types/model-v2";

interface ModelImportModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  isWorking?: boolean;
  onPreview: (
    input: ManagedModelImportV2Params,
  ) => Promise<ManagedModelImportPreviewV2Result | null>;
  onCommit: (
    input: ManagedModelImportV2Params,
  ) => Promise<ManagedModelImportPreviewV2Result | null>;
}

const EMPTY_PREVIEW: ManagedModelImportPreviewV2Result = {
  added: [],
  updated: [],
  conflicts: [],
  skipped: [],
  errors: [],
  ignoredFields: [],
  committed: 0,
};

const CONFLICT_STRATEGY_LABELS: Record<
  ManagedModelImportConflictStrategyV2,
  string
> = {
  keep_existing: "保留现有模型",
  replace_custom: "替换自定义模型",
};

function conflictStrategyLabel(
  strategy: ManagedModelImportConflictStrategyV2,
  t: (message: string) => string,
): string {
  return t(
    CONFLICT_STRATEGY_LABELS[strategy] || CONFLICT_STRATEGY_LABELS.keep_existing,
  );
}

function PreviewGroup({
  label,
  items,
  tone = "secondary",
}: {
  label: string;
  items: string[];
  tone?: "secondary" | "destructive" | "outline";
}) {
  if (items.length === 0) return null;
  return (
    <div className="space-y-1.5">
      <div className="flex items-center gap-2 text-xs font-medium">
        <span>{label}</span>
        <Badge variant={tone}>{items.length}</Badge>
      </div>
      <div className="max-h-24 overflow-y-auto rounded-md border border-border/60 bg-background/35 p-2 font-mono text-[11px] text-muted-foreground">
        {items.map((item) => (
          <div key={item} className="break-all py-0.5">{item}</div>
        ))}
      </div>
    </div>
  );
}

export function ModelImportModal({
  open,
  onOpenChange,
  isWorking = false,
  onPreview,
  onCommit,
}: ModelImportModalProps) {
  const { t } = useI18n();
  const [jsonContent, setJsonContent] = useState("");
  const [conflictStrategy, setConflictStrategy] =
    useState<ManagedModelImportConflictStrategyV2>("keep_existing");
  const [preview, setPreview] =
    useState<ManagedModelImportPreviewV2Result>(EMPTY_PREVIEW);
  const [error, setError] = useState<string | null>(null);

  const resetImportState = () => {
    setJsonContent("");
    setConflictStrategy("keep_existing");
    setPreview(EMPTY_PREVIEW);
    setError(null);
  };

  const handleOpenChange = (nextOpen: boolean) => {
    if (!nextOpen) {
      resetImportState();
    }
    onOpenChange(nextOpen);
  };

  const input = (): ManagedModelImportV2Params => ({
    jsonContent,
    conflictStrategy,
  });

  const handlePreview = async () => {
    if (!jsonContent.trim()) {
      setError(t("请选择或粘贴模型 JSON"));
      return;
    }
    setError(null);
    try {
      const result = await onPreview(input());
      if (result) setPreview(result);
    } catch (previewError) {
      setError(getAppErrorMessage(previewError));
    }
  };

  const handleCommit = async () => {
    setError(null);
    try {
      const result = await onCommit(input());
      if (result) {
        setPreview(result);
        handleOpenChange(false);
      }
    } catch (commitError) {
      setError(getAppErrorMessage(commitError));
    }
  };

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="glass-card p-0 sm:max-w-[760px]">
        <div className="max-h-[82vh] overflow-y-auto p-5">
          <DialogHeader>
            <DialogTitle>{t("导入到本地网关目录")}</DialogTitle>
            <DialogDescription>
              {t("支持模型目录导出格式和 Codex catalog 格式；所有导入项都会作为本地网关自定义模型处理，不影响直接连接 OpenAI 或使用官方目录的账号池。")}
            </DialogDescription>
          </DialogHeader>

          <div className="mt-5 space-y-4">
            <div className="space-y-2">
              <Label htmlFor="model-import-file">{t("本地 JSON 文件")}</Label>
              <Input
                id="model-import-file"
                type="file"
                accept="application/json,.json"
                onChange={(event) => {
                  const file = event.target.files?.[0];
                  if (!file) return;
                  void file
                    .text()
                    .then((content) => {
                      setJsonContent(content);
                      setPreview(EMPTY_PREVIEW);
                      setError(null);
                    })
                    .catch((fileError) => setError(getAppErrorMessage(fileError)));
                }}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="model-import-json">JSON</Label>
              <Textarea
                id="model-import-json"
                rows={12}
                className="font-mono text-xs"
                value={jsonContent}
                onChange={(event) => {
                  setJsonContent(event.target.value);
                  setPreview(EMPTY_PREVIEW);
                }}
                placeholder='{"models":[{"slug":"local-model","displayName":"Local Model"}]}'
              />
            </div>
            <div className="space-y-2">
              <Label>{t("冲突策略")}</Label>
              <Select
                value={conflictStrategy}
                onValueChange={(value) => {
                  setConflictStrategy(
                    (value || "keep_existing") as ManagedModelImportConflictStrategyV2,
                  );
                  setPreview(EMPTY_PREVIEW);
                }}
              >
                <SelectTrigger>
                  <SelectValue>
                    {(value) =>
                      conflictStrategyLabel(
                        (value ||
                          "keep_existing") as ManagedModelImportConflictStrategyV2,
                        t,
                      )
                    }
                  </SelectValue>
                </SelectTrigger>
                <SelectContent><SelectGroup>
                  <SelectItem value="keep_existing">
                    {conflictStrategyLabel("keep_existing", t)}
                  </SelectItem>
                  <SelectItem value="replace_custom">
                    {conflictStrategyLabel("replace_custom", t)}
                  </SelectItem>
                </SelectGroup></SelectContent>
              </Select>
            </div>

            <div className="flex justify-end">
              <Button type="button" variant="outline" disabled={isWorking} onClick={() => void handlePreview()}>
                {isWorking ? t("处理中...") : t("预览导入")}
              </Button>
            </div>

            {error ? <p className="text-sm text-destructive">{error}</p> : null}
            {preview.added.length + preview.updated.length + preview.conflicts.length + preview.skipped.length + preview.errors.length > 0 ? (
              <div className="grid gap-3 rounded-lg border border-border/60 bg-background/30 p-3 sm:grid-cols-2">
                <PreviewGroup label={t("新增")} items={preview.added} />
                <PreviewGroup label={t("更新")} items={preview.updated} />
                <PreviewGroup label={t("冲突")} items={preview.conflicts} tone="outline" />
                <PreviewGroup label={t("跳过")} items={preview.skipped} tone="outline" />
                <PreviewGroup label={t("错误")} items={preview.errors} tone="destructive" />
                <PreviewGroup label={t("忽略字段")} items={preview.ignoredFields} tone="outline" />
              </div>
            ) : null}
          </div>
        </div>

        <div className="border-t border-border/50 px-5 py-3">
          <DialogFooter>
            <DialogClose className={buttonVariants({ variant: "ghost" })} type="button">
              {t("取消")}
            </DialogClose>
            <Button
              type="button"
              disabled={
                isWorking ||
                !jsonContent.trim() ||
                preview.errors.length > 0 ||
                preview.added.length + preview.updated.length === 0
              }
              onClick={() => void handleCommit()}
            >
              {isWorking ? t("导入中...") : t("提交导入")}
            </Button>
          </DialogFooter>
        </div>
      </DialogContent>
    </Dialog>
  );
}
