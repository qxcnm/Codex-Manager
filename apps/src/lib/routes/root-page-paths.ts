export const ROOT_PAGE_PATHS = [
  "/",
  "/import",
  "/accounts",
  "/kiro",
  "/grok",
  "/account-manager",
  "/aggregate-api",
  "/apikeys",
  "/proxies",
  "/routing",
  "/models",
  "/model-groups",
  "/plugins",
  "/logs",
  "/settings",
  "/author",
] as const;

export type RootPagePath = (typeof ROOT_PAGE_PATHS)[number];



