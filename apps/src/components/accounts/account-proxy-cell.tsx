"use client";

import { useState } from "react";
import type { Account } from "@/types";
import { cn } from "@/lib/utils";
import { useI18n } from "@/lib/i18n/provider";
import {
	formatProxyGeoCountryLabel,
	formatProxyGeoTooltip,
	resolveProxyFlagDisplay,
} from "@/lib/utils/proxy-geo";
import {
	Tooltip,
	TooltipContent,
	TooltipTrigger,
} from "@/components/ui/tooltip";

export function ProxyFlag({
	countryCode,
	flagEmoji,
	flagImgUrl,
	className,
}: {
	countryCode?: string | null;
	flagEmoji?: string | null;
	flagImgUrl?: string | null;
	className?: string;
}) {
	const [hasError, setHasError] = useState(false);

	if (flagImgUrl && !hasError) {
		return (
			<img
				src={flagImgUrl}
				alt={countryCode || "flag"}
				className={cn("h-3 w-4 shrink-0 object-cover rounded-[1px]", className)}
				onError={() => setHasError(true)}
			/>
		);
	}

	const display = resolveProxyFlagDisplay(countryCode, flagEmoji);
	return <span className={className}>{display}</span>;
}

export function ProxyCountryFlag({
	countryCode,
	countryName,
	flagEmoji,
	flagImgUrl,
	className,
}: {
	countryCode?: string | null;
	countryName?: string | null;
	flagEmoji?: string | null;
	flagImgUrl?: string | null;
	className?: string;
}) {
	const { t } = useI18n();
	const label = formatProxyGeoCountryLabel(countryCode, countryName, t);

	return (
		<Tooltip>
			<TooltipTrigger
				render={<span />}
				className={cn("cursor-help", className)}
			>
				<ProxyFlag countryCode={countryCode} flagEmoji={flagEmoji} flagImgUrl={flagImgUrl} />
			</TooltipTrigger>
			<TooltipContent>{label}</TooltipContent>
		</Tooltip>
	);
}

function formatProxyUrlHost(urlStr?: string | null): string {
	if (!urlStr) return "";
	try {
		const parsed = new URL(urlStr);
		const host = parsed.hostname || "";
		if (!host) return urlStr;
		return parsed.port ? `${host}:${parsed.port}` : host;
	} catch {
		return urlStr || "";
	}
}

export function AccountProxyCell({ account }: { account: Account }) {
	const { t } = useI18n();
	const enabled = account.proxyEnabled === true;
	const ip = String(account.proxyIp || "").trim();
	const countryCode = account.proxyCountryCode || null;
	const countryName = account.proxyCountryName || null;
	const cityName = account.proxyCityName || null;
	const regionName = account.proxyRegionName || null;
	const flagEmoji = account.proxyFlagEmoji || null;
	const flagImgUrl = account.proxyFlagImgUrl || null;

	if (!enabled) {
		return <span className="text-muted-foreground">–</span>;
	}

	const displayIp = ip || formatProxyUrlHost(account.proxyUrl);
	const displayName = account.proxyProfileName || displayIp;

	if (!displayName) {
		return <span className="text-muted-foreground">–</span>;
	}

	return (
		<Tooltip>
			<TooltipTrigger render={<div />} className="min-w-0 cursor-help w-full">
				<div className="flex items-start gap-1.5 w-full">
					<ProxyFlag
						countryCode={countryCode}
						flagEmoji={flagEmoji}
						flagImgUrl={flagImgUrl}
						className="shrink-0 mt-0.5"
					/>
					<span className="min-w-0 text-xs leading-normal whitespace-normal break-words text-left" title={displayIp}>
						{displayName}
					</span>
				</div>
			</TooltipTrigger>
			<TooltipContent className="max-w-[280px] whitespace-pre-line">
				{formatProxyGeoTooltip(
					{
						ip: displayIp,
						countryCode,
						countryName,
						regionName,
						cityName,
						asn: account.proxyAsn,
						asOrg: account.proxyAsOrg,
						isp: account.proxyIsp,
						asDomain: account.proxyAsDomain,
						timezoneId: account.proxyTimezoneId,
						timezoneUtc: account.proxyTimezoneUtc,
					},
					t,
				)}
			</TooltipContent>
		</Tooltip>
	);
}
