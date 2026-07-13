export const ROOT_PAGE_PATHS = [
  "/",
  "/accounts",
  "/account-manager",
  "/aggregate-api",
  "/apikeys",
  "/models",
  "/model-groups",
  "/plugins",
  "/logs",
  "/settings",
] as const;

export type RootPagePath = (typeof ROOT_PAGE_PATHS)[number];
