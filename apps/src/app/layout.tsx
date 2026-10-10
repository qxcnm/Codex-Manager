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
  title: "CodexManager",
  description: "Account pool and usage management for Codex",
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
      lang="en"
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
