"use client";

import { Globe } from "lucide-react";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";
import { normalizeLocale } from "@/lib/i18n/config";
import { getLocaleLabel, useI18n } from "@/lib/i18n/provider";

interface LanguageSwitcherProps {
  className?: string;
  triggerClassName?: string;
  compact?: boolean;
}

export function LanguageSwitcher({
  className,
  triggerClassName,
  compact = false,
}: LanguageSwitcherProps) {
  const { locale, localeOptions, setLocale, isSwitchingLocale, t } = useI18n();

  return (
    <div className={cn("flex items-center gap-2", className)}>
      {!compact ? (
        <span className="text-xs font-medium text-muted-foreground">
          {t("界面语言")}
        </span>
      ) : null}
      <Select
        value={locale}
        onValueChange={(value) => void setLocale(normalizeLocale(value))}
        disabled={isSwitchingLocale}
      >
        <SelectTrigger
          className={cn("h-9 min-w-[116px] gap-2 text-xs", triggerClassName)}
          aria-label={t("选择语言")}
        >
          <div className="flex min-w-0 items-center justify-center gap-2">
            <Globe className="h-4 w-4 shrink-0 text-muted-foreground" />
            <span className={compact ? "hidden min-w-0 sm:inline" : "min-w-0"}>
              <SelectValue>
                {(value) => getLocaleLabel(normalizeLocale(value))}
              </SelectValue>
            </span>
          </div>
        </SelectTrigger>
        <SelectContent>
                    <SelectGroup>
          {localeOptions.map((item) => (
            <SelectItem key={item} value={item}>
              {getLocaleLabel(item)}
            </SelectItem>
          ))}
          </SelectGroup>
        </SelectContent>
      </Select>
    </div>
  );
}
