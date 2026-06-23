"use client";

import { useEffect, useMemo, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Loader2, Plus, Save } from "lucide-react";
import { toast } from "sonner";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button, buttonVariants } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  PROXY_PROFILES_QUERY_KEY,
  proxyProfilesClient,
} from "@/lib/api/proxy-profiles";
import { getAppErrorMessage } from "@/lib/api/transport";
import type { ProxyProfile } from "@/types";
import { useI18n } from "@/lib/i18n/provider";

interface ProxyProfileModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  profile?: ProxyProfile | null;
}

function parseTagsInput(value: string): string | null {
  const items = value
    .split(/[,\n]/)
    .map((item) => item.trim())
    .filter(Boolean);
  return items.length > 0 ? JSON.stringify(items) : null;
}

function stringifyTags(tagsJson?: string | null): string {
  if (!tagsJson) return "";
  try {
    const parsed = JSON.parse(tagsJson);
    return Array.isArray(parsed)
      ? parsed
          .map((item) => String(item || "").trim())
          .filter(Boolean)
          .join(", ")
      : "";
  } catch {
    return "";
  }
}

export function ProxyProfileModal({
  open,
  onOpenChange,
  profile,
}: ProxyProfileModalProps) {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const isEditing = Boolean(profile);
  const [name, setName] = useState("");
  const [proxyUrl, setProxyUrl] = useState("");
  const [tags, setTags] = useState("");
  const [enabled, setEnabled] = useState(true);
  const [notes, setNotes] = useState("");
  const [protocol, setProtocol] = useState<"http" | "https" | "socks5">("http");

  useEffect(() => {
    if (!open) return;
    setName(profile?.name || "");
    setProxyUrl("");
    setTags(stringifyTags(profile?.tagsJson));
    setEnabled(profile?.enabled ?? true);
    setNotes(profile?.notes || "");

    let defaultProto: "http" | "https" | "socks5" = "http";
    if (profile?.scheme) {
      const scheme = profile.scheme.toLowerCase();
      if (scheme === "https") {
        defaultProto = "https";
      } else if (scheme.startsWith("socks5")) {
        defaultProto = "socks5";
      }
    }
    setProtocol(defaultProto);
  }, [open, profile]);

  const actionLabel = useMemo(
    () => (isEditing ? t("保存更改") : t("添加代理")),
    [isEditing, t],
  );

  const saveMutation = useMutation({
    mutationFn: async () => {
      const trimmedName = name.trim();
      let trimmedProxyUrl = proxyUrl.trim();
      if (trimmedProxyUrl) {
        trimmedProxyUrl = trimmedProxyUrl.replace(/^[a-zA-Z0-9+-.]+:\/\//, "");
        const protoScheme = protocol === "socks5" ? "socks5h" : protocol;
        trimmedProxyUrl = `${protoScheme}://${trimmedProxyUrl}`;
      }
      if (!trimmedName) {
        throw new Error(t("名称不能为空"));
      }
      if (!profile && !trimmedProxyUrl) {
        throw new Error(t("代理 URL 不能为空"));
      }

      if (profile) {
        return proxyProfilesClient.updateProxyProfile({
          id: profile.id,
          name: trimmedName,
          proxyUrl: trimmedProxyUrl || null,
          enabled,
          tagsJson: parseTagsInput(tags),
          notes: notes.trim() || null,
        });
      }

      return proxyProfilesClient.createProxyProfile({
        name: trimmedName,
        proxyUrl: trimmedProxyUrl,
        enabled,
        tagsJson: parseTagsInput(tags),
        notes: notes.trim() || null,
      });
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({
        queryKey: PROXY_PROFILES_QUERY_KEY,
      });
      toast.success(isEditing ? t("代理已更新") : t("代理已创建"));
      onOpenChange(false);
    },
    onError: (error: unknown) => {
      toast.error(getAppErrorMessage(error));
    },
  });

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="glass-card max-h-[calc(100vh-2rem)] overflow-hidden p-0 sm:max-w-[640px]">
        <div className="flex max-h-[calc(100vh-2rem)] flex-col">
          <div className="border-b border-border/50 px-6 pt-6 pb-4">
            <DialogHeader>
              <DialogTitle>{isEditing ? t("编辑代理") : t("添加代理")}</DialogTitle>
            </DialogHeader>
          </div>

          <div className="grid gap-4 overflow-y-auto px-6 py-5">
            <div className="grid gap-4 sm:grid-cols-2">
              <div className="grid gap-2">
                <Label htmlFor="proxy-profile-name">{t("名称")}</Label>
                <Input
                  id="proxy-profile-name"
                  value={name}
                  disabled={saveMutation.isPending}
                  onChange={(event) => setName(event.target.value)}
                  placeholder={t("边缘中继")}
                />
              </div>
              <div className="rounded-xl border border-border/60 bg-muted/25 px-4 py-3">
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <Label htmlFor="proxy-profile-enabled" className="text-sm font-medium leading-none">{t("启用")}</Label>
                    <p className="mt-1.5 text-xs text-muted-foreground">
                      {t("禁用的配置会保留在列表中，以后可以重新启用。")}
                    </p>
                  </div>
                  <Switch
                    id="proxy-profile-enabled"
                    className="mt-0.5 shrink-0"
                    checked={enabled}
                    disabled={saveMutation.isPending}
                    onCheckedChange={(value) => setEnabled(Boolean(value))}
                  />
                </div>
              </div>
            </div>

            <div className="grid gap-2">
              <Label htmlFor="proxy-profile-url">{t("代理")}</Label>
              <div className="flex gap-2">
                <Select
                  value={protocol}
                  disabled={saveMutation.isPending}
                  onValueChange={(value) => setProtocol(value as any)}
                >
                  <SelectTrigger className="w-[110px] rounded-xl bg-card/50">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="http">HTTP</SelectItem>
                    <SelectItem value="https">HTTPS</SelectItem>
                    <SelectItem value="socks5">SOCKS5</SelectItem>
                  </SelectContent>
                </Select>
                <Input
                  id="proxy-profile-url"
                  className="flex-1"
                  value={proxyUrl}
                  disabled={saveMutation.isPending}
                  onChange={(event) => setProxyUrl(event.target.value)}
                  placeholder={
                    profile?.proxyUrlRedacted
                      ? profile.proxyUrlRedacted.replace(/^[a-zA-Z0-9+-.]+:\/\//, "")
                      : "127.0.0.1:7891"
                  }
                />
              </div>
              <p className="text-xs text-muted-foreground">
                {profile
                  ? t("留空以保持当前端点不变。")
                  : t("凭据以密文形式存储在后端，列表视图仅显示脱敏后的端点。")}
              </p>
            </div>

            <div className="grid gap-2">
              <Label htmlFor="proxy-profile-tags">{t("标签")}</Label>
              <Input
                id="proxy-profile-tags"
                value={tags}
                disabled={saveMutation.isPending}
                onChange={(event) => setTags(event.target.value)}
                placeholder="prod, us-west"
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="proxy-profile-notes">{t("备注（可选）")}</Label>
              <Textarea
                id="proxy-profile-notes"
                value={notes}
                disabled={saveMutation.isPending}
                onChange={(event) => setNotes(event.target.value)}
                placeholder={t("用于低延迟 OpenAI 出口。")}
                className="min-h-24"
              />
            </div>
          </div>

          <DialogFooter className="gap-2 border-t border-border/50 bg-muted/20 px-6 py-4 sm:items-center sm:justify-end sm:gap-2">
            <DialogClose
              className={buttonVariants({ variant: "outline" })}
              type="button"
              disabled={saveMutation.isPending}
            >
              {t("取消")}
            </DialogClose>
            <Button
              type="button"
              disabled={saveMutation.isPending}
              onClick={() => void saveMutation.mutateAsync()}
            >
              {saveMutation.isPending ? (
                <Loader2 data-icon="inline-start" className="animate-spin" />
              ) : isEditing ? (
                <Save data-icon="inline-start" />
              ) : null}
              {saveMutation.isPending ? t("保存中...") : actionLabel}
            </Button>
          </DialogFooter>
        </div>
      </DialogContent>
    </Dialog>
  );
}
