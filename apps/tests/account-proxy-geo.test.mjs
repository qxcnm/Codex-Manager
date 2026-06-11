import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";
import ts from "../node_modules/typescript/lib/typescript.js";

const appsRoot = path.resolve(import.meta.dirname, "..");

async function loadBundledModules() {
	const normalizePath = path.join(
		appsRoot,
		"src",
		"lib",
		"api",
		"account-proxy-normalize.ts",
	);
	const utilsPath = path.join(appsRoot, "src", "lib", "utils", "proxy-geo.ts");

	const normalizeSource = await fs.readFile(normalizePath, "utf8");
	const utilsSource = await fs.readFile(utilsPath, "utf8");

	// Combine sources and remove imports
	let combinedSource = utilsSource + "\n" + normalizeSource;
	combinedSource = combinedSource.replace(/import {.*?\} from ".*?";/g, "");

	const compiled = ts.transpileModule(combinedSource, {
		compilerOptions: {
			module: ts.ModuleKind.ES2022,
			target: ts.ScriptTarget.ES2022,
		},
	});

	const tempFile = path.join(appsRoot, "tests", ".temp-bundle.mjs");
	await fs.writeFile(tempFile, compiled.outputText, "utf8");
	const mod = await import(pathToFileURL(tempFile).href);
	await fs.unlink(tempFile);
	return mod;
}

const mod = await loadBundledModules();

test("normalizeAccountProxySummaryFields 映射 snake_case 字段到 camelCase", () => {
	const source = {
		proxy_enabled: true,
		proxy_status: "ok",
		proxy_ip: "1.1.1.1",
		proxy_country_code: "us",
		proxy_country_name: "United States",
		proxy_region_name: "California",
		proxy_city_name: "Los Angeles",
		proxy_geo_checked_at: 123456789,
	};

	const result = mod.normalizeAccountProxySummaryFields(source);

	assert.equal(result.proxyEnabled, true);
	assert.equal(result.proxyStatus, "ok");
	assert.equal(result.proxyIp, "1.1.1.1");
	assert.equal(result.proxyCountryCode, "US"); // Should be normalized to upper
	assert.equal(result.proxyCountryName, "United States");
	assert.equal(result.proxyRegionName, "California");
	assert.equal(result.proxyCityName, "Los Angeles");
	assert.equal(result.proxyGeoCheckedAt, 123456789);
});

test("countryCodeToFlag 转换 ISO 2 代码为 Emoji 旗帜", () => {
	assert.equal(mod.countryCodeToFlag("US"), "🇺🇸");
	assert.equal(mod.countryCodeToFlag("cn"), "🇨🇳");
	assert.equal(mod.countryCodeToFlag(null), "🌐");
});

test("formatProxyGeoCountryLabel 格式化国家显示", () => {
	assert.equal(
		mod.formatProxyGeoCountryLabel("US", "United States"),
		"United States (US)",
	);
	assert.equal(mod.formatProxyGeoCountryLabel("US"), "US");
	assert.equal(mod.formatProxyGeoCountryLabel(null, "Just Name"), "Just Name");
	assert.equal(mod.formatProxyGeoCountryLabel(), "Unknown");
});
