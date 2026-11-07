"use client";

import { useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { getAppErrorMessage } from "@/lib/api/transport";
import { useI18n } from "@/lib/i18n/provider";
import type { ServiceHealthReportInput } from "@/types/service-health";
import { PROVIDER_LABELS, REGION_LABELS, SYMPTOM_LABELS } from "./health-labels";

export function HealthReportDialog({ open, onOpenChange, onSubmit, pending }: {
  open: boolean; onOpenChange: (open: boolean) => void;
  onSubmit: (input: ServiceHealthReportInput) => Promise<unknown>; pending: boolean;
}) {
  const { t } = useI18n();
  const [input, setInput] = useState<ServiceHealthReportInput>({ provider: "codex", symptom: "unavailable", region: "unknown" });
  async function submit(event: React.FormEvent) {
    event.preventDefault();
    try {
      await onSubmit(input);
      toast.success(t("异常报告已提交"));
      onOpenChange(false);
    } catch (error) { toast.error(getAppErrorMessage(error)); }
  }
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("报告异常")}</DialogTitle>
          <DialogDescription>{t("仅共享异常类型与大致地区。报告 30 分钟后自动失效，恢复后可提前撤销。")}</DialogDescription>
        </DialogHeader>
        <form onSubmit={submit} className="flex flex-col gap-5">
          <FieldGroup>
            {([
              ["provider", "服务提供方", PROVIDER_LABELS],
              ["symptom", "异常现象", SYMPTOM_LABELS],
              ["region", "所在地区", REGION_LABELS],
            ] as const).map(([key, label, options]) => (
              <Field key={key}>
                <FieldLabel htmlFor={`health-${key}`}>{t(label)}</FieldLabel>
                <Select value={input[key]} onValueChange={(value) => { if (value && value in options) setInput((old) => ({ ...old, [key]: value })); }}>
                  <SelectTrigger id={`health-${key}`} className="w-full"><SelectValue>{t(options[input[key] as keyof typeof options])}</SelectValue></SelectTrigger>
                  <SelectContent><SelectGroup>{Object.entries(options).map(([value, title]) => <SelectItem key={value} value={value}>{t(title)}</SelectItem>)}</SelectGroup></SelectContent>
                </Select>
              </Field>
            ))}
          </FieldGroup>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>{t("取消")}</Button>
            <Button type="submit" disabled={pending}>{t(pending ? "提交中…" : "提交报告")}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
