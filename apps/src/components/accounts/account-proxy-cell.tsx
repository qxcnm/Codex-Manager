"use client";

import Image from "next/image";
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
			<Image
				src={flagImgUrl}
				alt={countryCode || "flag"}
				width={16}
				height={12}
				unoptimized
				className={cn("h-3.5 w-5 shrink-0 rounded-sm object-cover shadow-sm", className)}
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
				<div className="flex w-full items-start gap-2">
					<ProxyFlag
						countryCode={countryCode}
						flagEmoji={flagEmoji}
						flagImgUrl={flagImgUrl}
						className="mt-0.5 shrink-0 text-base leading-none"
					/>
					<span className="min-w-0 break-words text-left text-[13px] font-medium leading-5" title={displayIp}>
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
