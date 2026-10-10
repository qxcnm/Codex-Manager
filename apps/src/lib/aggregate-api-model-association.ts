export interface AggregateApiAssociationItem {
  upstreamModel: string;
  existingModelSlug: string | null;
}

export function sortAggregateApiAssociationItems<
  T extends AggregateApiAssociationItem,
>(items: readonly T[]): T[] {
  return items
    .map((item, index) => ({ item, index }))
    .sort((left, right) => {
      const existingDifference =
        Number(Boolean(right.item.existingModelSlug)) -
        Number(Boolean(left.item.existingModelSlug));
      return existingDifference || left.index - right.index;
    })
    .map(({ item }) => item);
}

export function existingAggregateApiModelIds(
  items: readonly AggregateApiAssociationItem[],
): string[] {
  return items
    .filter((item) => Boolean(item.existingModelSlug))
    .map((item) => item.upstreamModel);
}
