import { cn } from "@/lib/utils";

type ThemePreviewSwatchProps = {
  id: string;
  color: string;
  className?: string;
};

const DARK_THEME_IDS = new Set(["dark", "dark-one"]);
const THEME_PREVIEW_SURFACES: Record<string, { shell: string; panel: string }> = {
  tech: { shell: "#f7f9fd", panel: "#eef1fc" },
  dark: { shell: "#0b0d12", panel: "#1b1f29" },
  "dark-one": { shell: "#171a20", panel: "#2a303a" },
  business: { shell: "#fbfaf6", panel: "#f5ecd3" },
  mint: { shell: "#f7fbf9", panel: "#e6f4ee" },
  sunset: { shell: "#fcfaf7", panel: "#f5e9e1" },
  grape: { shell: "#faf9fd", panel: "#ede8f8" },
  ocean: { shell: "#f7fbfd", panel: "#e2f0f6" },
  forest: { shell: "#f8faf8", panel: "#e6eee8" },
  rose: { shell: "#fdf9fa", panel: "#f5e7ec" },
  slate: { shell: "#f8fafc", panel: "#e9edf2" },
  aurora: { shell: "#f7fbfb", panel: "#e2f1f0" },
};

export function ThemePreviewSwatch({
  id,
  color,
  className,
}: ThemePreviewSwatchProps) {
  const isDarkPreview = DARK_THEME_IDS.has(id);
  const surfaces = THEME_PREVIEW_SURFACES[id] ?? {
    shell: "#f8fafc",
    panel: "#eef2f7",
  };
  const subtleLine = isDarkPreview
    ? "rgba(255, 255, 255, 0.18)"
    : "rgba(15, 23, 42, 0.14)";
  const strongLine = isDarkPreview
    ? "rgba(255, 255, 255, 0.36)"
    : "rgba(15, 23, 42, 0.22)";

  return (
    <span
      className={cn(
        "relative block h-10 w-14 shrink-0 overflow-hidden rounded-md border shadow-sm",
        isDarkPreview ? "border-white/15" : "border-border/60",
        className,
      )}
      style={{
        background: `linear-gradient(135deg, ${surfaces.shell}, ${surfaces.panel})`,
      }}
      aria-hidden="true"
    >
      <span
        className="absolute inset-x-0 top-0 h-1"
        style={{ backgroundColor: color }}
      />
      <span
        className="absolute bottom-1.5 left-1.5 top-2 w-2 rounded-sm"
        style={{ backgroundColor: subtleLine }}
      />
      <span
        className="absolute left-5 right-1.5 top-2.5 h-1 rounded-full"
        style={{ backgroundColor: strongLine }}
      />
      <span
        className="absolute left-5 right-3 top-5 h-1 rounded-full"
        style={{ backgroundColor: color, opacity: 0.82 }}
      />
      <span
        className="absolute bottom-2 left-5 right-2 h-1 rounded-full"
        style={{ backgroundColor: subtleLine }}
      />
    </span>
  );
}
