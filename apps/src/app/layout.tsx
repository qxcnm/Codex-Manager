import type { Metadata } from "next";
import "./globals.css";
import { AppFrame } from "@/components/layout/app-frame";
import { Providers } from "@/components/providers";
import { AppBootstrap } from "@/components/layout/app-bootstrap";
import {
  appearanceInitScript,
  DEFAULT_APPEARANCE_PRESET,
} from "@/lib/appearance";

export const metadata: Metadata = {
  title: "OpenRuntime · One Runtime. Every AI.",
  description:
    "Build Once. Connect Every AI. An extensible AI Runtime and protocol adapter layer.",
};

const trayPreviewModeInitScript = `
(() => {
  try {
    if (window.location.pathname.replace(/\\/$/, "") === "/tray-preview") {
      document.documentElement.classList.add("tray-preview-mode");
    }
  } catch (_error) {}
})();
`;

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html
      lang="zh-CN"
      suppressHydrationWarning
      data-appearance={DEFAULT_APPEARANCE_PRESET}
    >
      <body className="antialiased">
        <script dangerouslySetInnerHTML={{ __html: trayPreviewModeInitScript }} />
        <script dangerouslySetInnerHTML={{ __html: appearanceInitScript }} />
        <Providers>
          <AppBootstrap>
            <AppFrame>{children}</AppFrame>
          </AppBootstrap>
        </Providers>
      </body>
    </html>
  );
}
