function normalizeAggregateApiProvider(value: string): string {
  return String(value || "")
    .trim()
    .toLowerCase()
    .replaceAll("-", "_");
}

export function aggregateApiUsesIncomingPath(providerType: string): boolean {
  return normalizeAggregateApiProvider(providerType) === "compatible";
}

export function aggregateApiProviderMatchesFilter(
  providerType: string,
  providerFilter: string,
): boolean {
  const provider = normalizeAggregateApiProvider(providerType);
  const filter = normalizeAggregateApiProvider(providerFilter);

  if (!filter || filter === "all" || provider === filter) return true;
  return provider === "compatible" && (filter === "codex" || filter === "claude");
}

export function aggregateApiStatusMatchesFilter(
  status: string,
  statusFilter: string,
): boolean {
  const normalizedStatus = normalizeAggregateApiProvider(status);
  const filter = normalizeAggregateApiProvider(statusFilter);

  return !filter || filter === "all" || normalizedStatus === filter;
}
