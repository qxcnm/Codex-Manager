"use client";

import {
	formatProxyGeoCountryLabel,
	formatProxyAsn,
	formatProxyTimezone,
} from "@/lib/utils/proxy-geo";
import { ProxyFlag } from "./account-proxy-cell";

export interface AccountProxyGeoStatus {
	ip?: string | null;
	countryCode?: string | null;
	countryName?: string | null;
	regionName?: string | null;
	cityName?: string | null;
	geoError?: string | null;

	asn?: number | null;
	asOrg?: string | null;
	isp?: string | null;
	asDomain?: string | null;
	timezoneId?: string | null;
	timezoneUtc?: string | null;
	flagEmoji?: string | null;
	flagImgUrl?: string | null;
}

export function AccountProxyGeoStatusGrid({
	geo,
	t,
}: {
	geo: AccountProxyGeoStatus | null | undefined;
	t: (key: string) => string;
}) {
	const countryCode = geo?.countryCode || null;
	const flagEmoji = geo?.flagEmoji || null;
	const flagImgUrl = geo?.flagImgUrl || null;

	const asnLabel = formatProxyAsn(geo?.asn);
	const timezoneLabel = formatProxyTimezone(geo?.timezoneId, geo?.timezoneUtc);

	const provider = geo?.asOrg || geo?.isp || "--";
	const isp = geo?.isp || geo?.asOrg || "--";
	const providerDomain = geo?.asDomain || "--";

	return (
		<div className="flex flex-col gap-4">
			<div className="grid gap-6 sm:grid-cols-2">
				{/* Столбец 1 */}
				<div className="flex flex-col gap-3">
					<div>
						<div className="text-[10px] font-medium text-muted-foreground/80 uppercase tracking-wider">
							{t("IP")}
						</div>
						<div className="mt-0.5 break-all font-mono text-xs font-normal text-foreground">
							{geo?.ip || "--"}
						</div>
					</div>
					<div>
						<div className="text-[10px] font-medium text-muted-foreground/80 uppercase tracking-wider">
							{t("国家")}
						</div>
						<div className="mt-0.5 flex flex-wrap items-center gap-1.5 text-xs font-normal text-foreground">
							<ProxyFlag
								countryCode={countryCode}
								flagEmoji={flagEmoji}
								flagImgUrl={flagImgUrl}
							/>
							<span>
								{geo?.countryName || geo?.countryCode
									? formatProxyGeoCountryLabel(geo?.countryCode, geo?.countryName)
									: "--"}
							</span>
						</div>
					</div>
					<div>
						<div className="text-[10px] font-medium text-muted-foreground/80 uppercase tracking-wider">
							{t("地区")}
						</div>
						<div className="mt-0.5 text-xs font-normal text-foreground">
							{geo?.regionName || "--"}
						</div>
					</div>
					<div>
						<div className="text-[10px] font-medium text-muted-foreground/80 uppercase tracking-wider">
							{t("城市")}
						</div>
						<div className="mt-0.5 truncate text-xs font-normal text-foreground" title={geo?.cityName || ""}>
							{geo?.cityName || "--"}
						</div>
					</div>
					<div>
						<div className="text-[10px] font-medium text-muted-foreground/80 uppercase tracking-wider">
							{t("Timezone")}
						</div>
						<div className="mt-0.5 truncate text-xs font-normal text-foreground" title={timezoneLabel || ""}>
							{timezoneLabel || "--"}
						</div>
					</div>
				</div>

				{/* Столбец 2 */}
				<div className="flex flex-col gap-3">
					<div>
						<div className="text-[10px] font-medium text-muted-foreground/80 uppercase tracking-wider">
							ASN
						</div>
						<div className="mt-0.5 text-xs font-normal text-foreground">
							{asnLabel || "--"}
						</div>
					</div>
					<div>
						<div className="text-[10px] font-medium text-muted-foreground/80 uppercase tracking-wider">
							{t("Provider")}
						</div>
						<div className="mt-0.5 truncate text-xs font-normal text-foreground" title={provider}>
							{provider}
						</div>
					</div>
					<div>
						<div className="text-[10px] font-medium text-muted-foreground/80 uppercase tracking-wider">
							ISP
						</div>
						<div className="mt-0.5 truncate text-xs font-normal text-foreground" title={isp}>
							{isp}
						</div>
					</div>
					<div>
						<div className="text-[10px] font-medium text-muted-foreground/80 uppercase tracking-wider">
							{t("Provider domain")}
						</div>
						<div className="mt-0.5 truncate text-xs font-normal text-foreground" title={providerDomain}>
							{providerDomain}
						</div>
					</div>
				</div>
			</div>

			{geo?.geoError ? (
				<div className="border-t border-border/50 pt-3">
					<div className="text-[10px] font-medium text-destructive uppercase tracking-wider">
						{t("地理位置错误")}
					</div>
					<div className="mt-0.5 break-words rounded-lg bg-destructive/10 px-2 py-1 font-mono text-xs text-destructive [overflow-wrap:anywhere]">
						{geo.geoError}
					</div>
				</div>
			) : null}
		</div>
	);
}
