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
import { normalizeLocalePreference } from "@/lib/i18n/config";
import { getLocaleLabel, useI18n } from "@/lib/i18n/provider";

interface LanguageSwitcherProps {
  className?: string;
  triggerClassName?: string;
  compact?: boolean;
  triggerId?: string;
  descriptionId?: string;
}

export function LanguageSwitcher({
  className,
  triggerClassName,
  compact = false,
  triggerId,
  descriptionId,
}: LanguageSwitcherProps) {
  const { locale, localePreference, localeOptions, setLocale, isSwitchingLocale, t } = useI18n();

  return (
    <div
      data-slot="language-switcher"
      className={cn("flex items-center gap-2", className)}
    >
      {!compact ? (
        <span className="text-xs font-medium text-muted-foreground">
          {t("界面语言")}
        </span>
      ) : null}
      <Select
        value={localePreference}
        onValueChange={(value) => void setLocale(normalizeLocalePreference(value))}
        disabled={isSwitchingLocale}
      >
        <SelectTrigger
          id={triggerId}
          aria-describedby={descriptionId}
          className={cn("h-9 min-w-[116px] gap-2 text-xs", triggerClassName)}
          aria-label={t("选择语言")}
        >
          <div className="flex min-w-0 flex-1 items-center justify-center gap-2 overflow-hidden">
            <Globe className="shrink-0 text-muted-foreground" />
            <span
              data-slot="language-switcher-label"
              className="flex min-w-0 flex-1 overflow-hidden"
            >
              <SelectValue className="min-w-0 truncate">
                {(value) => getLocaleLabel(normalizeLocalePreference(value), locale)}
              </SelectValue>
            </span>
          </div>
        </SelectTrigger>
        <SelectContent>
          <SelectGroup>
            {localeOptions.map((item) => (
              <SelectItem key={item} value={item}>
                {getLocaleLabel(item, locale)}
              </SelectItem>
            ))}
          </SelectGroup>
        </SelectContent>
      </Select>
    </div>
  );
}
