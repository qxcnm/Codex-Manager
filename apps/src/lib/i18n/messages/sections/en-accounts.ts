"use client";

import type { MessageCatalog } from "../types";

export const EN_ACCOUNTS_MESSAGES: MessageCatalog = {
	"5h 容量覆盖（Token）": "5h capacity override (tokens)",
	"7d 容量覆盖（Token）": "7d capacity override (tokens)",
	"AT/RT 刷新中...": "Refreshing AT/RT...",
	"AT/RT 刷新完成：成功{success}个，失败{failed}个，跳过{skipped}个":
		"AT/RT refresh completed: {success} succeeded, {failed} failed, {skipped} skipped",
	"AT/RT 刷新完成：成功{success}个，失败{failed}个，跳过{skipped}个；首个失败：{message}":
		"AT/RT refresh completed: {success} succeeded, {failed} failed, {skipped} skipped. First failure: {message}",
	"AT/RT 刷新完成：成功{success}个，跳过{skipped}个":
		"AT/RT refresh completed: {success} succeeded, {skipped} skipped",
	"AT/RT 过期、用量接口 401/403 等不可用账号":
		"Unavailable accounts such as expired AT/RT or usage API 401/403",
	"Refresh Token 失效，需要重新登录": "Refresh token is invalid. Log in again.",
	"Refresh Token 已被撤销，需要重新登录":
		"Refresh token was revoked. Log in again.",
	"Refresh Token 已被重复使用，需要重新登录":
		"Refresh token was reused. Log in again.",
	"Refresh Token 已过期，需要重新登录": "Refresh token expired. Log in again.",
	"Refresh Token 授权无效，需要重新登录":
		"Refresh token authorization is invalid. Log in again.",
	"仅用于额度池统计归属；留空表示该账号对全部 API 可用模型生效。":
		"Only used for quota pool ownership statistics. Leave blank to make this account effective for all API-available models.",
	"代理地区不受支持，已暂停账号刷新":
		"The proxy region is not supported. Account refresh has been paused.",
	保存账号代理: "Save account proxy",
	保存账号代理失败: "Failed to save account proxy",
	测试账号代理: "Test account proxy",
	测试账号代理失败: "Failed to test account proxy",
	测试: "Test",
	代理信息: "Proxy info",
	测试中: "Testing",
	未测试: "Untested",
	测试失败: "Test failed",
	测试状态: "Test status",
	可用: "Available",
	运行时错误: "Runtime error",
	从未检查: "Never checked",
	代理地址: "Proxy URL",
	地址无效: "Invalid URL",
	未配置: "Not configured",
	"刷新 AT/RT": "Refresh AT/RT",
	"刷新 AT/RT 失败": "Failed to refresh AT/RT",
	"刷新全部 AT/RT": "Refresh all AT/RT",
	刷新用量: "Refresh usage",
	"刷新登录凭证返回 401，需要重新登录":
		"Refreshing login credentials returned 401. Log in again.",
	读取账号代理: "Read account proxy",
	读取账号代理失败: "Failed to read account proxy",
	"服务未连接，暂时无法读取账号代理":
		"Service is not connected, account proxy cannot be read yet",
	保存: "Save",
	默认路由: "Default route",
	当前模式: "Current mode",
	清除: "Clear",
	错误: "Error",
	"启用后，该账号对 OpenAI API 的请求将通过指定的代理发送。":
		"When enabled, this account uses this proxy URL before other routing.",
	启用代理: "Enable proxy",
	启用账号代理: "Enable account proxy",
	清除账号代理: "Clear account proxy",
	清除账号代理失败: "Failed to clear account proxy",
	"为单个 OpenAI 账号配置本地代理。":
		"Configure a local proxy for one OpenAI account.",
	延迟: "Latency",
	原因码: "Reason code",
	容量覆盖: "Capacity override",
	当前没有匹配所选状态的账号: "No accounts match the selected statuses",
	当前没有可清理的账号: "No accounts available to clean",
	工作区已停用: "Workspace deactivated",
	"将删除所有匹配所选状态的账号，不再额外限制账号套餐。":
		"All accounts matching the selected statuses will be deleted without additionally filtering by account plan.",
	"已清理 {count} 个账号": "Cleaned {count} accounts",
	手动停用或旧版本标记的账号:
		"Accounts manually deactivated or marked by older versions",
	手动禁用的账号: "Manually disabled accounts",
	"批量刷新 AT/RT 失败": "Batch AT/RT refresh failed",
	按状态清理账号: "Clean accounts by status",
	"明确触发 usage_limit_reached 的账号，不包含低额度账号":
		"Accounts that explicitly triggered usage_limit_reached, excluding low-quota accounts",
	更多账号操作: "More account actions",
	未发现可清理的账号: "No cleanable accounts found",
	未设置账号容量覆盖: "No account capacity override set",
	状态原因: "Status reason",
	"用量接口返回 401，账号授权失效":
		"Usage API returned 401. Account authorization is invalid.",
	"用量接口返回 403，账号权限不足或被限制":
		"Usage API returned 403. Account lacks permission or is restricted.",
	"用量接口返回 HTTP {status}": "Usage API returned HTTP {status}",
	用量限制: "Usage limit",
	留空使用计划模板: "Leave blank to use the plan template",
	"的名称、标签、备注、排序与额度池配置。":
		"'s name, tags, notes, sort order, and quota pool configuration.",
	确认清理: "Confirm cleanup",
	"缺少授权 Token": "Missing auth token",
	"账号 AT/RT 已刷新": "Account AT/RT refreshed",
	账号代理: "Account proxy",
	账号代理测试通过: "Account proxy test passed",
	账号代理测试未通过: "Account proxy test did not pass",
	账号代理已保存: "Account proxy saved",
	账号代理已关闭: "Account proxy disabled",
	账号代理已清除: "Account proxy cleared",
	账号已停用: "Account deactivated",
	账号或工作区被停用的账号: "Accounts or workspaces that were deactivated",
	"状态字段为 unknown 的账号": "Accounts whose status field is unknown",
	"该账号只有用量快照，当前不能参与模型刷新或网关转发。请重新登录或刷新 AT/RT 后再使用。":
		"This account only has a usage snapshot and cannot participate in model refresh or gateway forwarding right now. Log in again or refresh AT/RT before using it.",
	请选择至少一种账号状态: "Select at least one account status",
	请至少选择一种账号状态: "Select at least one account status",
	"选择导出方式；如果已勾选账号，则只导出当前选中项。":
		"Choose an export mode. If accounts are selected, only the selected items will be exported.",
	"选择要删除的账号状态；删除后不可恢复。":
		"Choose the account statuses to delete. Deletion cannot be undone.",
	"建议登录、刷新、用量和 API 请求保持同一代理与地区，以降低账号风控和状态漂移。":
		"Use the same proxy and region for login, refresh, usage, and API requests to reduce account risk controls and state drift.",
	"支持 http、https、socks4、socks5；sing-box mixed inbound 通常填写 http://127.0.0.1:端口。":
		"Supports http, https, socks4 and socks5. For sing-box mixed inbound, usually enter http://127.0.0.1:port.",
	最近检查: "Last check",
	"这里展示账号套餐接口同步回来的套餐状态与时间信息。":
		"This shows plan status and time information synced from the account plan API.",
	"额度容量必须是大于 0 的数字，留空表示未覆盖":
		"Quota capacity must be a number greater than 0. Leave blank for no override.",
	额度已耗尽: "Quota exhausted",
	预计删除: "Estimated delete",
	代理: "Proxy",
	国家: "Country",
	国家名称: "Country Name",
	城市: "City",
	地区: "Region",
	地理位置错误: "Geolocation error",
	"模型白名单 / 额度管理": "Model Allowlist / Limit Management",
	未找到符合条件的账号: "No accounts found",
	未知: "Unknown",
	未知代理位置: "Unknown proxy location",
	未检查: "Unchecked",
	"Provider domain": "Provider domain",
	Provider: "Provider",
	ms: "ms",
	Timezone: "Timezone",
	IP: "IP",
};
