"use client";

import { useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import {
  Activity, Cable, CheckCircle2, CircleOff, Gauge, Globe2, Loader2,
  Network, Plus, RefreshCw, Save, Search, ShieldCheck, Trash2, Users,
  type LucideIcon,
} from "lucide-react";

import { accountClient } from "@/lib/api/account-client";
import {
  bindProxyAccounts, deleteProxyProfile, listProxyProfiles, probeProxyProfile,
  saveProxyProfile, type AccountProxyMode, type ProxyFallbackMode, type ProxyProfile,
} from "@/lib/api/proxy-profile-client";
import { getAppErrorMessage } from "@/lib/api/transport-errors";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { useI18n } from "@/lib/i18n/provider";

type Draft = {
  id?: string; name: string; proxyUrl: string; username: string; password: string;
  status: "active" | "disabled"; fallbackMode: ProxyFallbackMode; backupProxyId: string;
};

const emptyDraft = (): Draft => ({
  name: "", proxyUrl: "", username: "", password: "", status: "active",
  fallbackMode: "none", backupProxyId: "",
});

function formatTime(value: number | null) {
  return value ? new Date(value * 1000).toLocaleString() : "未探测";
}

function profileLabel(profile: ProxyProfile) {
  return profile.exitIp ? `${profile.name} · ${profile.exitIp}` : profile.name;
}

export default function ProxyProfilesPage() {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const [draft, setDraft] = useState<Draft>(emptyDraft);
  const [editing, setEditing] = useState(false);
  const [selectedAccounts, setSelectedAccounts] = useState<Set<string>>(new Set());
  const [bulkMode, setBulkMode] = useState<AccountProxyMode>("inherit");
  const [bulkProfileId, setBulkProfileId] = useState("");
  const [search, setSearch] = useState("");
  const [probingId, setProbingId] = useState<string | null>(null);

  const proxiesQuery = useQuery({ queryKey: ["proxy-profiles"], queryFn: listProxyProfiles });
  const accountsQuery = useQuery({ queryKey: ["accounts", "proxy-binding"], queryFn: () => accountClient.list() });
  const profiles = proxiesQuery.data?.items ?? [];
  const activeProfiles = profiles.filter((item) => item.status === "active");
  const bindings = useMemo(() => new Map((proxiesQuery.data?.bindings ?? []).map((item) => [item.accountId, item])), [proxiesQuery.data]);
  const accounts = useMemo(() => {
    const needle = search.trim().toLowerCase();
    return (accountsQuery.data?.items ?? []).filter((item) => !needle || item.label.toLowerCase().includes(needle));
  }, [accountsQuery.data, search]);

  const saveMutation = useMutation({
    mutationFn: () => saveProxyProfile({
      id: draft.id, name: draft.name, proxyUrl: draft.proxyUrl, username: draft.username || undefined,
      password: draft.password || undefined, keepExistingPassword: true, status: draft.status,
      fallbackMode: draft.fallbackMode, backupProxyId: draft.backupProxyId || undefined,
    }),
    onSuccess: () => {
      toast.success(t(draft.id ? "代理出口已更新" : "代理出口已添加"));
      setDraft(emptyDraft()); setEditing(false);
      void queryClient.invalidateQueries({ queryKey: ["proxy-profiles"] });
    },
    onError: (error) => toast.error(getAppErrorMessage(error)),
  });

  const bindMutation = useMutation({
    mutationFn: ({ ids, mode, profileId }: { ids: string[]; mode: AccountProxyMode; profileId?: string }) =>
      bindProxyAccounts(ids, mode, profileId),
    onSuccess: (result) => {
      toast.success(`${t("已更新")} ${result.updated} ${t("个账号的代理出口")}`);
      setSelectedAccounts(new Set());
      void queryClient.invalidateQueries({ queryKey: ["proxy-profiles"] });
    },
    onError: (error) => toast.error(getAppErrorMessage(error)),
  });

  const editProfile = (profile: ProxyProfile) => {
    setDraft({ id: profile.id, name: profile.name, proxyUrl: profile.proxyUrl,
      username: profile.proxyUsername ?? "", password: "", status: profile.status,
      fallbackMode: profile.fallbackMode, backupProxyId: profile.backupProxyId ?? "" });
    setEditing(true);
  };

  const runProbe = async (id: string) => {
    setProbingId(id);
    try {
      const result = await probeProxyProfile(id);
      toast[result.lastProbeStatus === "available" ? "success" : "error"](
        result.lastProbeStatus === "available" ? `${t("出口可用：")}${result.exitIp ?? t("已连通")}` : `${t("出口不可用：")}${result.lastProbeError ?? t("连接失败")}`,
      );
      await queryClient.invalidateQueries({ queryKey: ["proxy-profiles"] });
    } catch (error) { toast.error(getAppErrorMessage(error)); }
    finally { setProbingId(null); }
  };

  const totalBound = (proxiesQuery.data?.bindings ?? []).filter((item) => item.mode === "profile").length;
  const healthy = profiles.filter((item) => item.lastProbeStatus === "available").length;

  return (
    <div className="mx-auto flex w-full max-w-[1500px] flex-col gap-5 p-5 lg:p-7">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <div className="mb-2 flex items-center gap-2 text-xs font-semibold uppercase tracking-[0.18em] text-primary"><Network className="h-4 w-4" />{t("网络出口层")}</div>
          <h1 className="text-2xl font-semibold tracking-tight">{t("代理出口")}</h1>
          <p className="mt-1 text-sm text-muted-foreground">{t("账号探测、凭证刷新和正式调用固定使用同一个出口。")}</p>
        </div>
        <Button onClick={() => { setDraft(emptyDraft()); setEditing(true); }}><Plus className="mr-2 h-4 w-4" />{t("添加代理")}</Button>
      </div>

      <div className="grid gap-3 sm:grid-cols-3">
        {([
          { Icon: Globe2, label: "出口总数", value: profiles.length, detail: "HTTP / HTTPS / SOCKS5" },
          { Icon: ShieldCheck, label: "当前可用", value: healthy, detail: "以最近一次探测为准" },
          { Icon: Users, label: "固定绑定", value: totalBound, detail: "未绑定账号使用全局设置" },
        ] satisfies Array<{ Icon: LucideIcon; label: string; value: number; detail: string }>).map(({ Icon, label, value, detail }) => (
          <Card key={label} className="glass-card border-primary/15"><CardContent className="flex items-center gap-4 p-4">
            <div className="flex h-10 w-10 items-center justify-center rounded-xl border border-primary/20 bg-primary/10"><Icon className="h-5 w-5 text-primary" /></div>
            <div><p className="text-xs text-muted-foreground">{t(label)}</p><p className="text-xl font-semibold">{value}</p><p className="text-[11px] text-muted-foreground">{t(detail)}</p></div>
          </CardContent></Card>
        ))}
      </div>

      {editing && (
        <Card className="glass-card border-amber-300/30 shadow-[0_0_32px_-24px_rgba(251,191,36,.9)]">
          <CardHeader><CardTitle>{t(draft.id ? "编辑代理出口" : "添加代理出口")}</CardTitle><CardDescription>{t("密码会拆分并加密保存，不会显示在列表或日志中。")}</CardDescription></CardHeader>
          <CardContent className="grid gap-4 lg:grid-cols-4">
            <div className="grid gap-2"><Label>{t("名称")}</Label><Input value={draft.name} placeholder={t("例如：东京固定出口")} onChange={(e) => setDraft({ ...draft, name: e.target.value })} /></div>
            <div className="grid gap-2 lg:col-span-2"><Label>{t("代理地址")}</Label><Input value={draft.proxyUrl} placeholder={t("http://host:port 或 socks5://host:port")} onChange={(e) => setDraft({ ...draft, proxyUrl: e.target.value })} /></div>
            <div className="grid gap-2"><Label>{t("状态")}</Label><select className="h-9 rounded-md border bg-background px-3 text-sm" value={draft.status} onChange={(e) => setDraft({ ...draft, status: e.target.value as Draft["status"] })}><option value="active">{t("启用")}</option><option value="disabled">{t("停用")}</option></select></div>
            <div className="grid gap-2"><Label>{t("用户名")}</Label><Input value={draft.username} onChange={(e) => setDraft({ ...draft, username: e.target.value })} /></div>
            <div className="grid gap-2"><Label>{t("密码")}</Label><Input type="password" value={draft.password} placeholder={t(draft.id ? "留空保持原密码" : "可选")} onChange={(e) => setDraft({ ...draft, password: e.target.value })} /></div>
            <div className="grid gap-2"><Label>{t("失败后")}</Label><select className="h-9 rounded-md border bg-background px-3 text-sm" value={draft.fallbackMode} onChange={(e) => setDraft({ ...draft, fallbackMode: e.target.value as ProxyFallbackMode })}><option value="none">{t("停止使用，不切直连")}</option><option value="proxy">{t("切换备用代理")}</option><option value="direct">{t("允许切换直连")}</option></select></div>
            <div className="grid gap-2"><Label>{t("备用代理")}</Label><select disabled={draft.fallbackMode !== "proxy"} className="h-9 rounded-md border bg-background px-3 text-sm disabled:opacity-50" value={draft.backupProxyId} onChange={(e) => setDraft({ ...draft, backupProxyId: e.target.value })}><option value="">{t("请选择")}</option>{profiles.filter((item) => item.id !== draft.id).map((item) => <option key={item.id} value={item.id}>{profileLabel(item)}</option>)}</select></div>
            <div className="flex items-end gap-2 lg:col-span-4"><Button disabled={saveMutation.isPending} onClick={() => saveMutation.mutate()}>{saveMutation.isPending ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : <Save className="mr-2 h-4 w-4" />}{t("保存")}</Button><Button variant="outline" onClick={() => setEditing(false)}>{t("取消")}</Button></div>
          </CardContent>
        </Card>
      )}

      <Card className="glass-card">
        <CardHeader><CardTitle>{t("出口池")}</CardTitle><CardDescription>{t("固定账号与出口关系，避免同一账号在不同 IP 之间来回跳动。")}</CardDescription></CardHeader>
        <CardContent>
          {proxiesQuery.isLoading ? <div className="flex items-center gap-2 py-8 text-sm text-muted-foreground"><Loader2 className="h-4 w-4 animate-spin" />{t("正在读取代理出口")}</div> : profiles.length === 0 ? <div className="rounded-xl border border-dashed py-10 text-center text-sm text-muted-foreground">{t("尚未添加代理，账号继续使用“上游代理”或直连。")}</div> : (
            <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">{profiles.map((profile) => (
              <div key={profile.id} className="relative overflow-hidden rounded-xl border border-border/80 bg-background/55 p-4">
                <div className="absolute inset-x-8 top-0 h-px bg-gradient-to-r from-transparent via-amber-300/70 to-transparent" />
                <div className="flex items-start justify-between gap-3"><div><div className="flex items-center gap-2 font-medium">{profile.lastProbeStatus === "available" ? <CheckCircle2 className="h-4 w-4 text-emerald-500" /> : profile.lastProbeStatus === "failed" ? <CircleOff className="h-4 w-4 text-destructive" /> : <Activity className="h-4 w-4 text-muted-foreground" />}{profile.name}</div><p className="mt-1 max-w-[310px] truncate text-xs text-muted-foreground">{profile.proxyUrl}</p></div><Badge variant={profile.status === "active" ? "default" : "secondary"}>{profile.status === "active" ? "启用" : "停用"}</Badge></div>
                <div className="mt-4 grid grid-cols-3 gap-2 text-xs"><div className="rounded-lg bg-muted/50 p-2"><p className="text-muted-foreground">{t("出口 IP")}</p><p className="mt-1 truncate font-medium">{profile.exitIp ?? "—"}</p></div><div className="rounded-lg bg-muted/50 p-2"><p className="text-muted-foreground">{t("地区")}</p><p className="mt-1 truncate font-medium">{[profile.countryCode, profile.region].filter(Boolean).join(" · ") || "—"}</p></div><div className="rounded-lg bg-muted/50 p-2"><p className="text-muted-foreground">{t("延迟")}</p><p className="mt-1 font-medium">{profile.latencyMs == null ? "—" : `${profile.latencyMs} ms`}</p></div></div>
                {profile.lastProbeError && <p className="mt-3 line-clamp-2 text-xs text-destructive">{profile.lastProbeError}</p>}
                <p className="mt-3 text-[11px] text-muted-foreground">{t("最近探测：")}{formatTime(profile.lastProbeAt)}</p>
                <div className="mt-3 flex gap-2"><Button size="sm" variant="outline" disabled={probingId === profile.id} onClick={() => void runProbe(profile.id)}>{probingId === profile.id ? <Loader2 className="mr-1 h-3.5 w-3.5 animate-spin" /> : <Gauge className="mr-1 h-3.5 w-3.5" />}{t("立即探测")}</Button><Button size="sm" variant="ghost" onClick={() => editProfile(profile)}>{t("编辑")}</Button><Button size="sm" variant="ghost" className="ml-auto text-destructive" onClick={async () => { if (!window.confirm(`${t("删除代理“")}${profile.name}${t("”？绑定账号将恢复全局设置。")}`)) return; try { await deleteProxyProfile(profile.id); await queryClient.invalidateQueries({ queryKey: ["proxy-profiles"] }); toast.success(t("代理已删除")); } catch (error) { toast.error(getAppErrorMessage(error)); } }}><Trash2 className="h-3.5 w-3.5" /></Button></div>
              </div>
            ))}</div>
          )}
        </CardContent>
      </Card>

      <Card className="glass-card">
        <CardHeader><div className="flex flex-wrap items-center justify-between gap-3"><div><CardTitle>{t("Codex 账号出口绑定")}</CardTitle><CardDescription>{t("“跟随全局”使用系统设置里的上游代理；“固定出口”优先级最高。")}</CardDescription></div><div className="relative"><Search className="absolute left-3 top-2.5 h-4 w-4 text-muted-foreground" /><Input className="w-64 pl-9" value={search} placeholder={t("搜索账号")} onChange={(e) => setSearch(e.target.value)} /></div></div></CardHeader>
        <CardContent className="space-y-3">
          <div className="flex flex-wrap items-center gap-2 rounded-xl border bg-muted/25 p-3"><span className="text-sm font-medium">{t("已选")} {selectedAccounts.size} {t("个")}</span><select className="h-9 rounded-md border bg-background px-3 text-sm" value={bulkMode} onChange={(e) => setBulkMode(e.target.value as AccountProxyMode)}><option value="inherit">{t("跟随全局")}</option><option value="direct">{t("强制直连")}</option><option value="profile">{t("固定出口")}</option></select>{bulkMode === "profile" && <select className="h-9 rounded-md border bg-background px-3 text-sm" value={bulkProfileId} onChange={(e) => setBulkProfileId(e.target.value)}><option value="">{t("选择代理出口")}</option>{activeProfiles.map((item) => <option key={item.id} value={item.id}>{profileLabel(item)}</option>)}</select>}<Button size="sm" disabled={!selectedAccounts.size || bindMutation.isPending || (bulkMode === "profile" && !bulkProfileId)} onClick={() => bindMutation.mutate({ ids: [...selectedAccounts], mode: bulkMode, profileId: bulkProfileId })}>{bindMutation.isPending ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : <Cable className="mr-2 h-4 w-4" />}{t("应用绑定")}</Button><Button size="sm" variant="ghost" onClick={() => setSelectedAccounts(new Set(accounts.map((item) => item.id)))}>{t("全选当前")}</Button></div>
          <div className="divide-y overflow-hidden rounded-xl border">{accounts.map((account) => {
            const binding = bindings.get(account.id); const mode = binding?.mode ?? "inherit";
            return <div key={account.id} className="grid items-center gap-3 px-3 py-2.5 md:grid-cols-[28px_1fr_160px_260px]"><input type="checkbox" className="h-4 w-4 accent-primary" checked={selectedAccounts.has(account.id)} onChange={(e) => setSelectedAccounts((current) => { const next = new Set(current); e.target.checked ? next.add(account.id) : next.delete(account.id); return next; })} /><div className="min-w-0"><p className="truncate text-sm font-medium">{account.label}</p><p className="truncate text-[11px] text-muted-foreground">{account.planType ?? t("未知套餐")} · {account.gatewayProbeStatus ?? t("未探测")}</p></div><Badge variant={account.status === "active" ? "outline" : "secondary"}>{account.status === "active" ? t("账号可用") : account.status}</Badge><select aria-label={`${account.label} ${t("代理出口")}`} className="h-9 rounded-md border bg-background px-3 text-sm" value={mode === "profile" ? `profile:${binding?.proxyProfileId ?? ""}` : mode} onChange={(e) => { const value = e.target.value; const nextMode: AccountProxyMode = value.startsWith("profile:") ? "profile" : value as AccountProxyMode; const profileId = value.startsWith("profile:") ? value.slice(8) : undefined; bindMutation.mutate({ ids: [account.id], mode: nextMode, profileId }); }}><option value="inherit">{t("跟随全局")}</option><option value="direct">{t("强制直连")}</option>{profiles.map((item) => <option key={item.id} value={`profile:${item.id}`}>{t("固定：")}{profileLabel(item)}{item.status !== "active" ? t("（停用）") : ""}</option>)}</select></div>;
          })}{!accounts.length && <div className="py-8 text-center text-sm text-muted-foreground">{t("没有匹配的 Codex 账号")}</div>}</div>
        </CardContent>
      </Card>
    </div>
  );
}
