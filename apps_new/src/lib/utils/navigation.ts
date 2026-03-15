function trimTrailingSlash(pathname: string): string {
  return pathname.replace(/\/+$/, "");
}

export function normalizeAppPath(pathname: string | null | undefined): string {
  const rawValue = typeof pathname === "string" ? pathname.trim() : "";
  if (!rawValue || rawValue === "/") {
    return "/";
  }

  const [pathWithoutQuery] = rawValue.split("?");
  const withoutHtmlSuffix = pathWithoutQuery.replace(/\.html$/i, "");
  const normalized = trimTrailingSlash(withoutHtmlSuffix);

  if (!normalized || normalized === "/index") {
    return "/";
  }

  return normalized.startsWith("/") ? normalized : `/${normalized}`;
}

export function toStaticRouteHref(
  pathname: string,
  searchParams?: URLSearchParams | string
): string {
  const normalizedPath = normalizeAppPath(pathname);
  const queryString =
    searchParams instanceof URLSearchParams
      ? searchParams.toString()
      : typeof searchParams === "string"
        ? searchParams.replace(/^\?/, "")
        : "";

  const routePath = normalizedPath === "/" ? "/" : `${normalizedPath}/`;
  return queryString ? `${routePath}?${queryString}` : routePath;
}
