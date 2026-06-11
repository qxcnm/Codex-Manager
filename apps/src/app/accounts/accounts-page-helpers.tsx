"use client";

import type { LucideIcon } from "lucide-react";
import { Power, PowerOff, RefreshCw, Zap } from "lucide-react";
import { useI18n } from "@/lib/i18n/provider";
import { cn } from "@/lib/utils";
import {
	formatRemainingDurationFromSeconds,
	formatTsFromSeconds,
	getExtraUsageDisplayRows,
	getUsageDisplayBuckets,
	isPrimaryWindowOnlyUsage,
	isSecondaryWindowOnlyUsage,
} from "@/lib/utils/usage";
import { Badge } from "@/components/ui/badge";
import { Progress } from "@/components/ui/progress";
import {
	Tooltip,
	TooltipContent,
	TooltipTrigger,
} from "@/components/ui/tooltip";
import type { Account } from "@/types";

export type StatusFilter =
	| "all"
	| "available"
	| "low_quota"
	| "limited"
	| "banned";
export type AccountExportMode = "single" | "multiple";
export type AccountSizeSortMode = "large-first" | "small-first";

const ACCOUNT_SORT_STEP = 5;

export function fitLongTextClassName(
	value: string,
	baseClassName: string,
	defaultSizeClassName: string,
): string {
	const length = Array.from(String(value || "")).length;
	if (length > 96) return cn(baseClassName, "text-[8px] leading-tight");
	if (length > 72) return cn(baseClassName, "text-[9px] leading-snug");
	if (length > 40) return cn(baseClassName, "text-[10px] leading-snug");
	if (length > 24) return cn(baseClassName, "text-[11px] leading-snug");
	return cn(baseClassName, defaultSizeClassName);
}

export type TranslateFn = (
	key: string,
	values?: Record<string, string | number>,
) => string;

export function formatAccountPlanValueLabel(value: string, t: TranslateFn) {
	const normalized = String(value || "")
		.trim()
		.toLowerCase();
	switch (normalized) {
		case "free":
			return "FREE";
		case "go":
			return "GO";
		case "plus":
			return "PLUS";
		case "pro":
			return "PRO";
		case "team":
			return "TEAM";
		case "business":
			return "BUSINESS";
		case "enterprise":
			return "ENTERPRISE";
		case "edu":
			return "EDU";
		case "unknown":
			return t("未知");
		default:
			return normalized ? normalized.toUpperCase() : t("未知");
	}
}

export function normalizeAccountPlanKey(account: Account) {
	return (
		String(account.planType || "")
			.trim()
			.toLowerCase() || "unknown"
	);
}

export function formatPlanFilterLabel(value: string, t: TranslateFn) {
	const nextValue = String(value || "").trim();
	if (!nextValue || nextValue === "all") {
		return t("全部类型");
	}
	return formatAccountPlanValueLabel(nextValue, t);
}

export function formatStatusFilterLabel(value: string, t: TranslateFn) {
	const nextValue = String(value || "").trim();
	switch (nextValue) {
		case "available":
			return t("可用");
		case "low_quota":
			return t("低配额");
		case "limited":
			return t("限流");
		case "banned":
			return t("封禁");
		case "all":
		default:
			return t("全部");
	}
}

export interface QuotaProgressProps {
	label: string;
	remainPercent: number | null;
	resetsAt: number | null;
	icon: LucideIcon;
	tone: "green" | "blue" | "amber";
	caption?: string;
	emptyText?: string;
	emptyResetText?: string;
}

export interface QuotaSummaryItem extends QuotaProgressProps {
	id: string;
}

export interface AccountEditorState {
	accountId: string;
	accountName: string;
	currentLabel: string;
	currentTags: string;
	currentNote: string;
	currentSort: number;
	currentModelSlugs: string;
	currentQuotaPrimaryWindowTokens: number | null;
	currentQuotaSecondaryWindowTokens: number | null;
}

export type DeleteDialogState =
	| { kind: "single"; account: Account }
	| { kind: "selected"; ids: string[]; count: number }
	| null;

export const QUOTA_TONE_CLASSES = {
	blue: {
		track: "bg-blue-500/20",
		indicator: "bg-blue-500",
		icon: "text-blue-500",
	},
	green: {
		track: "bg-green-500/20",
		indicator: "bg-green-500",
		icon: "text-green-500",
	},
	amber: {
		track: "bg-amber-500/20",
		indicator: "bg-amber-500",
		icon: "text-amber-500",
	},
} as const;


function QuotaProgress({
	label,
	remainPercent,
	resetsAt,
	icon: Icon,
	tone,
	caption,
	emptyText = "--",
	emptyResetText = "未知",
}: QuotaProgressProps) {
	const { t } = useI18n();
	const value = remainPercent ?? 0;
	const centerText = caption ?? label;
	const showLabelOnRight = Boolean(caption);
	const palette = QUOTA_TONE_CLASSES[tone];

	return (
		<div className="flex min-w-[180px] flex-col gap-1.5">
			<div className="flex items-center justify-between gap-2 text-[10px]">
				<div className="flex min-w-0 flex-1 items-center gap-1.5">
					<Icon className={cn("h-3 w-3 shrink-0", palette.icon)} />
					{caption ? (
						<span className="shrink-0 whitespace-nowrap font-medium text-foreground/80">
							{label}
						</span>
					) : null}
					<span
						className={fitLongTextClassName(
							caption ?? label,
							"min-w-0 flex-1 truncate text-center text-muted-foreground/80",
							"text-[10px]",
						)}
						title={caption ?? label}
					>
						{caption ?? label}
					</span>
				</div>
				<div className="flex shrink-0 items-center gap-2 font-medium">
					<span>{remainPercent == null ? emptyText : `${value}%`}</span>
				</div>
			</div>
			<Progress
				value={value}
				trackClassName={palette.track}
				indicatorClassName={palette.indicator}
			/>
			<div className="text-[10px] text-muted-foreground">
				{t("重置")}: {formatTsFromSeconds(resetsAt, emptyResetText)}
			</div>
		</div>
	);
}

export function QuotaOverviewCell({ items }: { items: QuotaSummaryItem[] }) {
	const { locale, t } = useI18n();
	const summaryItems = items.slice(0, 2);

	return (
		<Tooltip>
			<TooltipTrigger render={<div />} className="block min-w-0 cursor-help">
				<div className="rounded-xl border border-primary/5 bg-accent/10 px-3 py-2">
					<div className="space-y-3">
						{summaryItems.map((item) => {
							const palette = QUOTA_TONE_CLASSES[item.tone];
							return (
								<div key={item.id} className="min-w-0 space-y-1.5">
									<div className="flex items-center justify-between gap-2 text-[10px]">
										<div className="flex min-w-0 flex-1 items-center gap-1.5 text-muted-foreground">
											{item.icon && (
												<item.icon className={cn("h-3.5 w-3.5 shrink-0", palette.icon)} />
											)}
											{item.caption ? (
												<span className="shrink-0 whitespace-nowrap font-medium text-foreground/80">
													{item.label}
												</span>
											) : null}
											<span
												className={fitLongTextClassName(
													item.caption ?? item.label,
													"min-w-0 flex-1 truncate text-center [overflow-wrap:anywhere]",
													"text-[10px]",
												)}
												title={item.caption ?? item.label}
											>
												{item.caption ?? item.label}
											</span>
										</div>
										<div className="flex shrink-0 items-center gap-2 font-medium text-foreground/80 text-[10px]">
											<span>
											{item.remainPercent == null
												? (item.emptyText ?? "--")
												: `${item.remainPercent}%`}
											</span>
										</div>
									</div>
									<Progress
										value={item.remainPercent ?? 0}
										trackClassName={palette.track}
										indicatorClassName={palette.indicator}
									/>
								<div className="flex min-w-0 items-center justify-between gap-2 text-[9px] text-muted-foreground/80 pt-0.5 leading-none">
									<span
										className={fitLongTextClassName(
											`${t("重置")}: ${formatTsFromSeconds(
												item.resetsAt,
												item.emptyResetText ?? t("未知"),
											)}`,
											"min-w-0 break-words [overflow-wrap:anywhere]",
											"text-[9px]",
										)}
									>
										{t("重置")}: {formatTsFromSeconds(
											item.resetsAt,
											item.emptyResetText ?? t("未知"),
										)}
									</span>
									<span className="shrink-0 whitespace-nowrap text-[9px]">
										{formatRemainingDurationFromSeconds(
											item.resetsAt,
											item.id.endsWith("-primary") ? "hours" : "days",
											item.emptyResetText ?? t("未知"),
											locale,
										)}
									</span>
								</div>
							</div>
							);
						})}
					</div>
				</div>
			</TooltipTrigger>
			<TooltipContent
				side="right"
				align="center"
				sideOffset={10}
				className="max-w-[340px] rounded-lg bg-popover p-3 text-popover-foreground shadow-md"
			>
				<div className="space-y-3">
					<div className="space-y-1">
						<p className="text-sm font-semibold">
							{t("额度详情（悬停查看所有额度）")}
						</p>
						<p className="text-[10px] text-muted-foreground">
							{t("标准额度与专属额度统一在这里查看。")}
						</p>
					</div>
					<div className="space-y-2">
						{items.map((item) => (
							<QuotaProgress
								key={item.id}
								label={item.label}
								remainPercent={item.remainPercent}
								resetsAt={item.resetsAt}
								icon={item.icon}
								tone={item.tone}
								caption={item.caption}
								emptyText={item.emptyText}
								emptyResetText={item.emptyResetText}
							/>
						))}
					</div>
				</div>
			</TooltipContent>
		</Tooltip>
	);
}

export function getAccountStatusAction(
	account: Account,
	t: TranslateFn,
): {
	action: "enable" | "disable" | null;
	label: string;
	icon: LucideIcon;
} {
	const normalizedStatus = String(account.status || "")
		.trim()
		.toLowerCase();
	if (normalizedStatus === "disabled") {
		return { action: "enable", label: t("启用账号"), icon: Power };
	}
	if (normalizedStatus === "inactive") {
		return { action: "enable", label: t("恢复账号"), icon: Power };
	}
	if (normalizedStatus === "banned") {
		return { action: null, label: t("封禁账号"), icon: PowerOff };
	}
	return { action: "disable", label: t("禁用账号"), icon: PowerOff };
}

export function getAccountStatusReasonCode(account: Account): string {
	const reason = String(account.statusReason || "").trim();
	return reason.toLowerCase() === "usage_ok" ? "" : reason;
}

export function formatAccountStatusReasonLabel(
	account: Account,
	t: TranslateFn,
): string | null {
	const reasonCode = getAccountStatusReasonCode(account);
	if (!reasonCode) {
		return null;
	}

	const reason = reasonCode.toLowerCase();
	if (reason.startsWith("refresh_token_invalid:")) {
		const detail = reason.slice("refresh_token_invalid:".length);
		switch (detail) {
			case "refresh_token_reused":
				return t("Refresh Token 已被重复使用，需要重新登录");
			case "refresh_token_invalidated":
				return t("Refresh Token 已被撤销，需要重新登录");
			case "refresh_token_expired":
				return t("Refresh Token 已过期，需要重新登录");
			case "invalid_grant":
				return t("Refresh Token 授权无效，需要重新登录");
			case "refresh_token_unknown_401":
				return t("刷新登录凭证返回 401，需要重新登录");
			default:
				return t("Refresh Token 失效，需要重新登录");
		}
	}
	if (reason === "refresh_token_region_blocked") {
		return t("代理地区不受支持，已暂停账号刷新");
	}

	const usageHttpStatus = reason.match(/^usage_http_(\d{3})$/);
	if (usageHttpStatus) {
		const statusCode = usageHttpStatus[1];
		if (statusCode === "401") {
			return t("用量接口返回 401，账号授权失效");
		}
		if (statusCode === "403") {
			return t("用量接口返回 403，账号权限不足或被限制");
		}
		return t("用量接口返回 HTTP {status}", { status: statusCode });
	}

	switch (reason) {
		case "account_deactivated":
			return t("账号已停用");
		case "workspace_deactivated":
		case "deactivated_workspace":
			return t("工作区已停用");
		case "usage_limit_exhausted":
			return t("额度已耗尽");
		default:
			return reasonCode;
	}
}

export function AccountStatusCell({ account }: { account: Account }) {
	const { t } = useI18n();
	const statusReasonCode = getAccountStatusReasonCode(account);
	const statusReasonLabel = formatAccountStatusReasonLabel(account, t);
	const statusText = t(account.availabilityText || "未知");

	return (
		<Tooltip>
			<TooltipTrigger render={<div />} className="block min-w-0 cursor-help">
				<div className="flex min-w-0 flex-col gap-1">
					<div className="flex min-w-0 items-center gap-1.5">
						<div
							className={cn(
								"h-1.5 w-1.5 shrink-0 rounded-full",
								account.isAvailable ? "bg-green-500" : "bg-red-500",
							)}
						/>
						<span
							className={cn(
								"text-[11px] font-medium",
								account.isAvailable
									? "text-green-600 dark:text-green-400"
									: "text-red-600 dark:text-red-400",
							)}
						>
							{statusText}
						</span>
					</div>
					{statusReasonLabel ? (
						<span
							className={fitLongTextClassName(
								statusReasonLabel,
								"block max-w-[180px] whitespace-normal break-words text-muted-foreground [overflow-wrap:anywhere]",
								"text-[10px] leading-snug",
							)}
							title={statusReasonLabel}
						>
							{statusReasonLabel}
						</span>
					) : null}
				</div>
			</TooltipTrigger>
			<TooltipContent
				side="left"
				align="center"
				className="max-w-[320px] bg-popover p-3 text-popover-foreground shadow-md"
			>
				<div className="space-y-2 text-left">
					<div className="space-y-0.5">
						<div className="text-[10px] text-muted-foreground">
							{t("当前状态")}
						</div>
						<div className="font-medium">{statusText}</div>
					</div>
					{statusReasonLabel ? (
						<div className="space-y-0.5">
							<div className="text-[10px] text-muted-foreground">
								{t("状态原因")}
							</div>
							<div className="font-medium">{statusReasonLabel}</div>
						</div>
					) : null}
					{statusReasonCode ? (
						<div className="space-y-0.5">
							<div className="text-[10px] text-muted-foreground">
								{t("原因码")}
							</div>
							<div className="break-all rounded-md bg-muted px-2 py-1 font-mono text-[10px]">
								{statusReasonCode}
							</div>
						</div>
					) : null}
				</div>
			</TooltipContent>
		</Tooltip>
	);
}

export function formatAccountPlanLabel(
	account: Account,
	t: TranslateFn,
): string | null {
	const normalized = normalizeAccountPlanKey(account);
	return normalized === "unknown"
		? null
		: formatAccountPlanValueLabel(normalized, t);
}

export function formatAccountSubscriptionPlanLabel(
	account: Account,
	t: TranslateFn,
): string {
	const normalized = String(account.subscriptionPlan || account.planType || "")
		.trim()
		.toLowerCase();
	return normalized ? formatAccountPlanValueLabel(normalized, t) : t("未知");
}

export function formatAccountSubscriptionStatusLabel(
	account: Account,
	t: TranslateFn,
): string {
	const hasSubscriptionEvidence =
		Boolean(String(account.subscriptionPlan || "").trim()) ||
		account.subscriptionExpiresAt != null ||
		account.subscriptionRenewsAt != null;
	const nowSeconds = Math.floor(Date.now() / 1000);
	const isExpired =
		account.subscriptionExpiresAt != null &&
		account.subscriptionExpiresAt < nowSeconds &&
		Boolean(String(account.subscriptionPlan || account.planType || "").trim());

	if (
		account.hasSubscription === true ||
		(account.hasSubscription == null && hasSubscriptionEvidence)
	) {
		return t("已订阅");
	}
	if (isExpired) {
		return t("已过期");
	}
	if (account.hasSubscription === false) {
		return t("未订阅");
	}
	return t("未知");
}

export function getAccountPlanBadgeClassName(planLabel: string | null): string {
	switch (planLabel) {
		case "FREE":
			return "bg-slate-500/10 text-slate-700 dark:text-slate-300";
		case "GO":
			return "bg-sky-500/10 text-sky-700 dark:text-sky-300";
		case "PLUS":
			return "bg-amber-500/10 text-amber-700 dark:text-amber-300";
		case "PRO":
			return "bg-fuchsia-500/10 text-fuchsia-700 dark:text-fuchsia-300";
		case "TEAM":
			return "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300";
		case "BUSINESS":
			return "bg-indigo-500/10 text-indigo-700 dark:text-indigo-300";
		case "ENTERPRISE":
			return "bg-rose-500/10 text-rose-700 dark:text-rose-300";
		case "EDU":
			return "bg-cyan-500/10 text-cyan-700 dark:text-cyan-300";
		default:
			return "bg-accent/50";
	}
}

export function formatAccountTags(tags: string[]): string {
	return tags
		.map((tag) => String(tag || "").trim())
		.filter(Boolean)
		.join("、");
}

export function normalizeTagsDraft(tagsDraft: string): string[] {
	return tagsDraft
		.split(",")
		.map((tag) => tag.trim())
		.filter(Boolean);
}

export function buildAccountOrderUpdates(orderedAccounts: Account[]) {
	return orderedAccounts.reduce<Array<{ accountId: string; sort: number }>>(
		(updates, account, index) => {
			const nextSort = index * ACCOUNT_SORT_STEP;
			const currentSort = Number.isFinite(account.priority)
				? account.priority
				: Number(account.sort) || 0;
			if (currentSort !== nextSort) {
				updates.push({ accountId: account.id, sort: nextSort });
			}
			return updates;
		},
		[],
	);
}

export function getAccountSizeGroup(
	account: Account,
): "large" | "standard" | "small" {
	switch (normalizeAccountPlanKey(account)) {
		case "plus":
		case "pro":
		case "team":
		case "business":
		case "enterprise":
			return "large";
		case "free":
			return "small";
		default:
			return "standard";
	}
}

export function buildAccountsBySizeOrder(
	orderedAccounts: Account[],
	mode: AccountSizeSortMode,
) {
	const buckets = {
		large: [] as Account[],
		standard: [] as Account[],
		small: [] as Account[],
	};

	for (const account of orderedAccounts) {
		buckets[getAccountSizeGroup(account)].push(account);
	}

	return mode === "large-first"
		? [...buckets.large, ...buckets.standard, ...buckets.small]
		: [...buckets.small, ...buckets.standard, ...buckets.large];
}

export function formatAccountExportModeLabel(value: string, t: TranslateFn) {
	return value === "single" ? t("单 JSON") : t("多 JSON");
}

export function buildQuotaSummaryItems(
	account: Account,
	t: TranslateFn,
): QuotaSummaryItem[] {
	const primaryWindowOnly = isPrimaryWindowOnlyUsage(account.usage);
	const secondaryWindowOnly = isSecondaryWindowOnlyUsage(account.usage);
	const usageBuckets = getUsageDisplayBuckets(account.usage);
	const extraUsageRows = getExtraUsageDisplayRows(account.usage);
	return [
		{
			id: `${account.id}-primary`,
			label: t("5小时"),
			remainPercent: account.primaryRemainPercent,
			resetsAt: usageBuckets.primaryResetsAt,
			icon: RefreshCw,
			tone: "green",
			caption: t("标准模型窗口"),
			emptyText: secondaryWindowOnly ? t("未提供") : "--",
			emptyResetText: secondaryWindowOnly ? t("未提供") : t("未知"),
		},
		{
			id: `${account.id}-secondary`,
			label: t("7天"),
			remainPercent: account.secondaryRemainPercent,
			resetsAt: usageBuckets.secondaryResetsAt,
			icon: RefreshCw,
			tone: "blue",
			caption: t("长周期窗口"),
			emptyText: primaryWindowOnly ? t("未提供") : "--",
			emptyResetText: primaryWindowOnly ? t("未提供") : t("未知"),
		},
		...extraUsageRows.map((item) => ({
			id: item.id,
			label: `${t(item.label, item.labelValues)}${item.labelSuffix ? t(item.labelSuffix) : ""}`,
			remainPercent: item.remainPercent,
			resetsAt: item.resetsAt,
			icon: Zap,
			tone: "amber" as const,
			caption: t(item.windowLabel, item.windowLabelValues),
			emptyText: "--",
			emptyResetText: t("未知"),
		})),
	];
}

export function AccountInfoCell({
	account,
	isPreferred,
}: {
	account: Account;
	isPreferred: boolean;
}) {
	const { t } = useI18n();
	const accountPlanLabel = formatAccountPlanLabel(account, t);
	const subscriptionStatusLabel = formatAccountSubscriptionStatusLabel(
		account,
		t,
	);
	const subscriptionPlanLabel = formatAccountSubscriptionPlanLabel(account, t);
	const subscriptionExpiryText =
		account.subscriptionExpiresAt != null
			? formatTsFromSeconds(account.subscriptionExpiresAt, t("未知"))
			: account.hasSubscription === false
				? t("未订阅")
				: t("未知");
	const statusReasonCode = getAccountStatusReasonCode(account);
	const statusReasonLabel = formatAccountStatusReasonLabel(account, t);
	const tagsText = formatAccountTags(account.tags);
	const noteText = String(account.note || "").trim();

	return (
		<Tooltip>
			<TooltipTrigger
				render={<div />}
				className="block min-w-0 max-w-full cursor-help text-left"
			>
				<div className="flex min-w-0 flex-col whitespace-normal">
					<div className="flex min-w-0 flex-wrap items-center gap-1.5">
						<span
							className={fitLongTextClassName(
								account.name,
								"inline-block min-w-0 max-w-full break-words font-semibold [overflow-wrap:anywhere]",
								"text-sm",
							)}
							title={account.name}
						>
							{account.name}
						</span>
						{accountPlanLabel ? (
							<Badge
								variant="secondary"
								className={cn(
									"h-4 shrink-0 px-1.5 text-[9px]",
									getAccountPlanBadgeClassName(accountPlanLabel),
								)}
							>
								{accountPlanLabel}
							</Badge>
						) : null}
						{isPreferred ? (
							<Badge
								variant="secondary"
								className="h-4 shrink-0 bg-amber-500/15 px-1.5 text-[9px] text-amber-700 dark:text-amber-300"
							>
								{t("优先")}
							</Badge>
						) : null}
					</div>
					<span
						className={fitLongTextClassName(
							account.id,
							"mt-0.5 block max-w-full break-all font-mono uppercase text-muted-foreground opacity-60 [overflow-wrap:anywhere]",
							"text-[10px]",
						)}
						title={account.id}
					>
						{account.id}
					</span>
					<span className="mt-1 text-[10px] text-muted-foreground">
						{t("最近刷新")}:{" "}
						{formatTsFromSeconds(account.lastRefreshAt, t("从未刷新"))}
					</span>
					<span className="text-[10px] text-muted-foreground">
						{t("订阅到期")}: {subscriptionExpiryText}
					</span>
				</div>
			</TooltipTrigger>
			<TooltipContent className="max-w-sm">
				<div className="flex min-w-[260px] flex-col gap-2">
					<div className="grid gap-2 sm:grid-cols-2">
						<div className="space-y-0.5">
							<div className="text-[10px] text-background/70">
								{t("账号类型")}
							</div>
							<div className="font-medium">{accountPlanLabel || t("未知")}</div>
						</div>
						<div className="space-y-0.5">
							<div className="text-[10px] text-background/70">
								{t("当前状态")}
							</div>
							<div className="font-medium">
								{t(account.availabilityText || "未知")}
							</div>
						</div>
						{statusReasonLabel ? (
							<div className="space-y-0.5 sm:col-span-2">
								<div className="text-[10px] text-background/70">
									{t("状态原因")}
								</div>
								<div className="font-medium">{statusReasonLabel}</div>
								{statusReasonCode ? (
									<div className="break-all font-mono text-[10px] text-background/70">
										{statusReasonCode}
									</div>
								) : null}
							</div>
						) : null}
						<div className="space-y-0.5">
							<div className="text-[10px] text-background/70">
								{t("订阅状态")}
							</div>
							<div className="font-medium">{subscriptionStatusLabel}</div>
						</div>
						<div className="space-y-0.5">
							<div className="text-[10px] text-background/70">
								{t("订阅方案")}
							</div>
							<div className="font-medium">{subscriptionPlanLabel}</div>
						</div>
					</div>
					<div className="grid gap-2 sm:grid-cols-2">
						<div className="space-y-0.5">
							<div className="text-[10px] text-background/70">
								{t("到期时间")}
							</div>
							<div className="font-medium">
								{formatTsFromSeconds(account.subscriptionExpiresAt, t("未知"))}
							</div>
						</div>
						<div className="space-y-0.5">
							<div className="text-[10px] text-background/70">
								{t("续费时间")}
							</div>
							<div className="font-medium">
								{formatTsFromSeconds(account.subscriptionRenewsAt, t("未知"))}
							</div>
						</div>
					</div>
					<div className="space-y-0.5">
						<div className="text-[10px] text-background/70">{t("标签")}</div>
						<div className="break-words">{tagsText || t("未设置")}</div>
					</div>
					<div className="space-y-0.5">
						<div className="text-[10px] text-background/70">{t("备注")}</div>
						<div className="whitespace-pre-wrap break-words">
							{noteText || t("未设置")}
						</div>
					</div>
					<div className="space-y-0.5">
						<div className="text-[10px] text-background/70">{t("账号 ID")}</div>
						<div className="break-all font-mono text-[11px]">{account.id}</div>
					</div>
				</div>
			</TooltipContent>
		</Tooltip>
	);
}
