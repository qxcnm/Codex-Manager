"use client";

import { useQuery } from "@tanstack/react-query";
import { Field, FieldDescription, FieldError, FieldLabel } from "@/components/ui/field";
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { PROXY_PROFILES_QUERY_KEY, proxyProfilesClient } from "@/lib/api/proxy-profiles";
import { useI18n } from "@/lib/i18n/provider";

interface AccountLoginProxySelectProps {
  enabled: boolean;
  disabled: boolean;
  serviceAddr: string;
  value: string;
  onValueChange: (value: string) => void;
}

export function AccountLoginProxySelect({
  enabled, disabled, serviceAddr, value, onValueChange,
}: AccountLoginProxySelectProps) {
  const { t } = useI18n();
  const profilesQuery = useQuery({
    queryKey: [...PROXY_PROFILES_QUERY_KEY, serviceAddr],
    queryFn: () => proxyProfilesClient.listProxyProfiles(),
    enabled,
    refetchOnMount: "always",
  });
  const profiles = profilesQuery.data?.items ?? [];
  const selected = profiles.find((profile) => profile.id === value);

  return (
    <Field>
      <FieldLabel htmlFor="account-login-proxy">{t("登录与账号代理")}</FieldLabel>
      <Select
        value={value || "__default__"}
        disabled={disabled || profilesQuery.isLoading}
        onValueChange={(next) => onValueChange(!next || next === "__default__" ? "" : next)}
      >
        <SelectTrigger id="account-login-proxy" className="h-10 w-full">
          <SelectValue>
            {value ? selected?.name || t("已删除的代理配置") : t("不指定（沿用默认网络）")}
          </SelectValue>
        </SelectTrigger>
        <SelectContent>
          <SelectGroup>
          <SelectItem value="__default__">{t("不指定（沿用默认网络）")}</SelectItem>
          {value && !selected ? (
            <SelectItem value={value} disabled>{t("已删除的代理配置")}</SelectItem>
          ) : null}
          {profiles.map((profile) => (
            <SelectItem key={profile.id} value={profile.id} disabled={!profile.enabled}>
              {profile.name}{profile.ip ? ` · ${profile.ip}` : ""}
              {!profile.enabled ? ` (${t("已禁用")})` : ""}
            </SelectItem>
          ))}
          </SelectGroup>
        </SelectContent>
      </Select>
      <FieldDescription>
        {t("所选代理用于服务端登录请求，登录成功后保存为账号代理。浏览器授权页面仍使用浏览器自身的网络设置。")}
      </FieldDescription>
      {profilesQuery.isError ? (
        <FieldError>{t("加载代理配置失败，请稍后重新打开弹窗。")}</FieldError>
      ) : enabled && !profilesQuery.isLoading && profiles.length === 0 ? (
        <FieldDescription>{t("暂无可用代理配置，请先在设置中添加。")}</FieldDescription>
      ) : null}
    </Field>
  );
}
