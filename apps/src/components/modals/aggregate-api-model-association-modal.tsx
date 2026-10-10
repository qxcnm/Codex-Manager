"use client";

import { useMemo, useState } from "react";
import { Link, ListChecks, ListX, Search, SearchX } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button, buttonVariants } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  existingAggregateApiModelIds,
  sortAggregateApiAssociationItems,
} from "@/lib/aggregate-api-model-association";
import { useI18n } from "@/lib/i18n/provider";
import type { AggregateApi } from "@/types/api-key";

interface ModelAssociationItem {
  upstreamModel: string;
  displayName: string | null;
  existingModelSlug: string | null;
  alreadyLinked: boolean;
}

interface AggregateApiModelAssociationModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  aggregateApi?: AggregateApi | null;
  sourceName?: string | null;
  items: ModelAssociationItem[];
  isSaving?: boolean;
  onAssociate: (upstreamModels: string[]) => Promise<void>;
}

interface ModelAssociationDraft {
  items: ModelAssociationItem[];
  search: string;
  selected: Set<string>;
}

export function AggregateApiModelAssociationModal({
  open,
  onOpenChange,
  aggregateApi,
  sourceName,
  items,
  isSaving = false,
  onAssociate,
}: AggregateApiModelAssociationModalProps) {
  const { t } = useI18n();
  const [draft, setDraft] = useState<ModelAssociationDraft>(() => ({
    items,
    search: "",
    selected: new Set(items.map((item) => item.upstreamModel)),
  }));
  const draftMatchesItems = draft.items === items;
  const search = draftMatchesItems ? draft.search : "";
  const selected = draftMatchesItems
    ? draft.selected
    : new Set(items.map((item) => item.upstreamModel));

  const setSearch = (value: string) => {
    setDraft({ items, search: value, selected });
  };

  const setSelected = (next: Set<string>) => {
    setDraft({ items, search, selected: next });
  };

  const filteredItems = useMemo(() => {
    const query = search.trim().toLowerCase();
    const sortedItems = sortAggregateApiAssociationItems(items);
    if (!query) return sortedItems;
    return sortedItems.filter((item) =>
      `${item.upstreamModel} ${item.displayName || ""}`.toLowerCase().includes(query),
    );
  }, [items, search]);
  const existingModelIds = useMemo(
    () => existingAggregateApiModelIds(items),
    [items],
  );

  const allSelected = items.length > 0 && items.every((item) => selected.has(item.upstreamModel));
  const selectedCount = selected.size;
  const supplierName =
    sourceName?.trim() ||
    aggregateApi?.supplierName?.trim() ||
    aggregateApi?.url ||
    t("聚合 API");

  const toggle = (model: string, checked: boolean) => {
    const next = new Set(selected);
    if (checked) next.add(model);
    else next.delete(model);
    setSelected(next);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="glass-card flex max-h-[90dvh] w-[calc(100%-2rem)] max-w-[calc(100%-2rem)] flex-col gap-0 overflow-hidden p-0 sm:max-w-[860px] lg:max-w-[920px]">
        <DialogHeader className="shrink-0 border-b border-border/50 px-5 pb-4 pt-5 sm:px-6 sm:pt-6">
          <div className="flex items-start gap-3 pr-8">
            <div className="mt-0.5 flex size-10 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary ring-1 ring-primary/15">
              <Link className="h-5 w-5" />
            </div>
            <div className="min-w-0">
              <DialogTitle className="truncate text-lg sm:text-xl">
                {t("关联目录模型")}
              </DialogTitle>
              <DialogDescription className="mt-1 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs sm:text-sm">
                <span className="max-w-[360px] truncate font-medium text-foreground/80">{supplierName}</span>
                <span aria-hidden="true" className="text-border">·</span>
                <span>{t("拉取到 {count} 个上游模型", { count: items.length })}</span>
                <Badge variant="outline" className="ml-1 h-5 px-1.5 text-[10px] font-medium uppercase">
                  {t("模型目录 V2")}
                </Badge>
              </DialogDescription>
            </div>
          </div>
        </DialogHeader>

        <div className="flex min-h-0 flex-1 flex-col overflow-hidden px-5 py-4 sm:px-6">
          <div className="flex flex-col gap-3 rounded-lg border border-border/60 bg-background/35 p-3 sm:flex-row sm:items-center">
            <div className="relative min-w-0 flex-1">
              <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
              <Input
                value={search}
                onChange={(event) => setSearch(event.target.value)}
                placeholder={t("搜索模型")}
                aria-label={t("搜索模型")}
                className="h-9 border-border/60 bg-background/55 pl-9"
              />
            </div>
            <div className="flex flex-wrap items-center justify-between gap-2 sm:justify-end">
              <span className="text-xs text-muted-foreground">
                {t("显示 {shown} / {total} 个", { shown: filteredItems.length, total: items.length })}
                <span className="mx-1 text-border">·</span>
                <span className="font-medium text-foreground/75">{t("已选择 {count} 个", { count: selectedCount })}</span>
              </span>
              <Button
                type="button"
                size="sm"
                variant="outline"
                className="h-9 shrink-0 bg-background/45"
                disabled={existingModelIds.length === 0}
                onClick={() => setSelected(new Set(existingModelIds))}
              >
                <ListChecks className="mr-1.5 h-4 w-4" />
                {t("选择已有模型")}
              </Button>
              <Button
                type="button"
                size="sm"
                variant="outline"
                className="h-9 shrink-0 bg-background/45"
                onClick={() => setSelected(allSelected ? new Set() : new Set(items.map((item) => item.upstreamModel)))}
              >
                {allSelected ? <ListX className="mr-1.5 h-4 w-4" /> : <ListChecks className="mr-1.5 h-4 w-4" />}
                {allSelected ? t("取消全选") : t("全选")}
              </Button>
            </div>
          </div>

          <div className="mt-4 flex min-h-0 flex-1 flex-col overflow-hidden rounded-lg border border-border/60 bg-background/25">
            <div className="flex shrink-0 items-center justify-between border-b border-border/50 bg-muted/20 px-3 py-2 text-[11px] font-medium uppercase text-muted-foreground sm:px-4">
              <span>{t("上游模型")}</span>
              <span>{t("状态")}</span>
            </div>
            <div className="min-h-0 flex-1 overflow-y-auto [scrollbar-color:var(--border)_transparent] [scrollbar-width:thin]">
              {filteredItems.length === 0 ? (
                <div className="flex min-h-44 flex-col items-center justify-center gap-2 px-4 text-center text-sm text-muted-foreground">
                  <SearchX className="h-5 w-5 opacity-60" />
                  <span>{t("没有匹配的模型")}</span>
                </div>
              ) : (
                filteredItems.map((item) => (
                  <label
                    key={item.upstreamModel}
                    className="group grid min-h-[62px] cursor-pointer grid-cols-[auto_minmax(0,1fr)] items-center gap-x-3 gap-y-2 border-b border-border/40 px-3 py-3 transition-colors last:border-b-0 hover:bg-primary/[0.04] sm:grid-cols-[auto_minmax(0,1fr)_auto] sm:px-4"
                  >
                    <Checkbox
                      checked={selected.has(item.upstreamModel)}
                      onCheckedChange={(checked) => toggle(item.upstreamModel, checked === true)}
                      aria-label={item.upstreamModel}
                    />
                    <span className="min-w-0 flex-1">
                      <span className="block truncate font-mono text-sm font-medium text-foreground/90">
                        {item.upstreamModel}
                      </span>
                      <span className="mt-0.5 block truncate text-xs text-muted-foreground">
                        {item.displayName && item.displayName !== item.upstreamModel
                          ? item.displayName
                          : t("将使用上游模型 ID 作为显示名")}
                      </span>
                    </span>
                    <span className="col-start-2 flex min-w-0 flex-wrap items-center gap-1.5 sm:col-start-3 sm:row-start-1 sm:justify-end">
                      {item.existingModelSlug ? <Badge variant="secondary" className="h-6 px-2 text-[11px]">{t("已有模型")}</Badge> : null}
                      {item.alreadyLinked ? <Badge variant="outline" className="h-6 border-primary/25 px-2 text-[11px] text-primary">{t("已关联")}</Badge> : null}
                      {!item.existingModelSlug && !item.alreadyLinked ? <span className="text-[11px] text-muted-foreground">{t("待关联")}</span> : null}
                    </span>
                  </label>
                ))
              )}
            </div>
          </div>
        </div>

        <DialogFooter className="mx-0 mb-0 shrink-0 border-t border-border/50 bg-muted/15 px-5 py-3 sm:px-6">
          <div className="mr-auto hidden text-xs text-muted-foreground sm:block">
            {t("已选择 {count} 个模型", { count: selectedCount })}
          </div>
          <DialogClose className={buttonVariants({ variant: "ghost" })} type="button" disabled={isSaving}>
            {t("取消")}
          </DialogClose>
          <Button
            type="button"
            className="min-w-[180px]"
            onClick={() => void onAssociate([...selected])}
            disabled={isSaving || selectedCount === 0}
          >
            <Link className="mr-1.5 h-4 w-4" />
            {isSaving ? t("关联中...") : t("关联所选模型 ({count})", { count: selectedCount })}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
