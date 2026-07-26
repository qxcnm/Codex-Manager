import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";

const appsRoot = path.resolve(import.meta.dirname, "..");

test("sidebar collapse avoids transitions that continuously reflow the active page", async () => {
  const source = await fs.readFile(
    path.join(appsRoot, "src", "components", "layout", "sidebar.tsx"),
    "utf8",
  );

  assert.match(
    source,
    /isSidebarOpen \? "w-\[220px\] xl:w-\[280px\]" : "w-\[60px\] xl:w-\[72px\]"/,
  );
  assert.match(
    source,
    /data-slot="app-sidebar"[\s\S]{0,220}flex shrink-0 flex-col glass-sidebar/,
  );
  assert.doesNotMatch(source, /transition-\[width\]/);
  assert.doesNotMatch(source, /transition-all/);
  assert.doesNotMatch(source, /transition-\[clip-path\]/);
  assert.doesNotMatch(source, /will-change:clip-path/);
  assert.doesNotMatch(source, /app-sidebar-motion-(?:layer|surface)/);
  assert.match(
    source,
    /data-slot="app-sidebar-motion-edge"[\s\S]{0,300}transition-transform/,
  );
});

test("main content exposes a stable layout target for sidebar regression checks", async () => {
  const source = await fs.readFile(
    path.join(appsRoot, "src", "components", "layout", "app-frame.tsx"),
    "utf8",
  );

  assert.match(source, /data-slot="app-main-column"/);
  assert.match(source, /data-command-center="true"/);
  assert.doesNotMatch(source, /data-command-center=\{currentShellPath ===/);
});

test("compact sidebar renders every dynamically allowed route without a fixed menu list", async () => {
  const source = await fs.readFile(
    path.join(appsRoot, "src", "components", "layout", "sidebar.tsx"),
    "utf8",
  );

  assert.match(source, /getAllowedTopLevelRoutes\(routeAccess\)\.flatMap/);
  assert.doesNotMatch(source, /COMMAND_CENTER_PATHS/);
  assert.doesNotMatch(source, /commandCenterItems/);
  assert.doesNotMatch(source, /currentShellPath === "\/"\s*\?/);
});

test("command center shell follows every configured theme", async () => {
  const source = await fs.readFile(
    path.join(appsRoot, "src", "app", "globals.css"),
    "utf8",
  );

  const commandCenterStyles = source.slice(
    source.indexOf(".routing-command-card"),
    source.indexOf(".console-panel"),
  );

  assert.match(commandCenterStyles, /--command-center-accent: var\(--primary\)/);
  assert.match(commandCenterStyles, /rgb\(var\(--primary-rgb\) \/ 0\.085\)/);
  assert.match(commandCenterStyles, /background: var\(--console-sidebar\)/);
  assert.match(commandCenterStyles, /background: var\(--console-header\)/);
  assert.match(commandCenterStyles, /color: var\(--primary-foreground\) !important/);
  assert.match(
    commandCenterStyles,
    /body\.low-transparency \.console-shell\[data-command-center='true'\][\s\S]*background: var\(--bg-color\)/,
  );
  assert.doesNotMatch(commandCenterStyles, /--command-center-accent: #[0-9a-f]{3,8}/i);
  assert.doesNotMatch(commandCenterStyles, /#f8f9ff/i);
});

test("theme palettes and previews stay synchronized", async () => {
  const [cssSource, themeListSource, previewSource] = await Promise.all([
    fs.readFile(path.join(appsRoot, "src", "app", "globals.css"), "utf8"),
    fs.readFile(
      path.join(appsRoot, "src", "app", "settings", "settings-page-helpers.ts"),
      "utf8",
    ),
    fs.readFile(
      path.join(
        appsRoot,
        "src",
        "app",
        "settings",
        "components",
        "theme-preview-swatch.tsx",
      ),
      "utf8",
    ),
  ]);

  const themes = [...themeListSource.matchAll(/\{ id: "([^"]+)", name: "[^"]+", color: "(#[0-9a-f]{6})" \}/gi)];
  assert.equal(themes.length, 12);

  for (const [, id, color] of themes) {
    const escapedId = id.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    assert.match(
      cssSource,
      new RegExp(`\\[data-theme='${escapedId}'\\][\\s\\S]{0,700}--primary: ${color}`, "i"),
    );
    assert.match(
      previewSource,
      new RegExp(`(?:^|\\n)\\s*(?:"${escapedId}"|${escapedId}): \\{ shell:`, "m"),
    );
  }

  assert.doesNotMatch(cssSource, /--background: #f0fdf4/);
  assert.doesNotMatch(cssSource, /--background: #fff1f2/);
});

test("gradient appearance keeps surfaces subtle in desktop and web shells", async () => {
  const [cssSource, layoutSource, webBuildSource] = await Promise.all([
    fs.readFile(path.join(appsRoot, "src", "app", "globals.css"), "utf8"),
    fs.readFile(path.join(appsRoot, "src", "app", "layout.tsx"), "utf8"),
    fs.readFile(path.join(appsRoot, "..", "crates", "web", "build.rs"), "utf8"),
  ]);

  const modernTheme = cssSource.slice(
    cssSource.indexOf("[data-appearance='modern'] {"),
    cssSource.indexOf("[data-appearance='classic'] body"),
  );
  assert.match(modernTheme, /rgb\(var\(--surface-rgb\) \/ 0\.76\) 90%/);
  assert.match(modernTheme, /--glass-surface: color-mix\(in srgb, var\(--card\) 86%, transparent\)/);
  assert.match(modernTheme, /0 8px 20px -20px rgb\(var\(--primary-rgb\) \/ 0\.14\)/);
  assert.doesNotMatch(modernTheme, /rgb\(var\(--bg-gradient-1-rgb\) \/ 1\) 24%/);

  assert.match(cssSource, /\[data-appearance='modern'\] \.dashboard-primary-panel[\s\S]{0,360}var\(--card\) 72%/);
  assert.match(cssSource, /\[data-appearance='modern'\] \.dashboard-analytics-card \.mission-panel[\s\S]{0,260}var\(--card\) 34%/);

  assert.match(layoutSource, /<AppFrame>\{children\}<\/AppFrame>/);
  assert.match(webBuildSource, /join\("\.\.\/\.\.\/apps\/out"\)/);
});

test("narrow viewports start with the sidebar collapsed", async () => {
  const source = await fs.readFile(
    path.join(appsRoot, "src", "components", "layout", "app-frame.tsx"),
    "utf8",
  );

  assert.match(source, /NARROW_VIEWPORT_QUERY = "\(max-width: 639px\)"/);
  assert.match(source, /if \(narrowViewport\.matches\) \{\s*setSidebarOpen\(false\);/);
  assert.match(source, /narrowViewport\.addEventListener\("change", collapseSidebar\)/);
  assert.match(source, /narrowViewport\.removeEventListener\("change", collapseSidebar\)/);
});

test("wide but short windows keep the complete sidebar discoverable", async () => {
  const source = await fs.readFile(
    path.join(appsRoot, "src", "components", "layout", "sidebar.tsx"),
    "utf8",
  );

  assert.match(source, /\[@media\(max-height:800px\)\]:min-h-10/);
  assert.match(source, /\[@media\(max-height:800px\)\]:h-\[68px\]/);
  assert.match(source, /\[@media\(max-height:800px\)\]:py-3/);
});

test("page fallback stays aligned with both sidebar widths", async () => {
  const source = await fs.readFile(
    path.join(
      appsRoot,
      "src",
      "components",
      "layout",
      "page-keep-alive-viewport.tsx",
    ),
    "utf8",
  );

  assert.match(
    source,
    /isSidebarOpen \? "left-\[220px\] xl:left-\[280px\]" : "left-\[60px\] xl:left-\[72px\]"/,
  );
});
