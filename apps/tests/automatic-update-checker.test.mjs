import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";

const appsRoot = path.resolve(import.meta.dirname, "..");
const checkerSource = await fs.readFile(
  path.join(
    appsRoot,
    "src",
    "components",
    "layout",
    "automatic-update-checker.tsx",
  ),
  "utf8",
);
const bootstrapSource = await fs.readFile(
  path.join(appsRoot, "src", "components", "layout", "app-bootstrap.tsx"),
  "utf8",
);
const settingsCardSource = await fs.readFile(
  path.join(
    appsRoot,
    "src",
    "app",
    "settings",
    "components",
    "general-basics-card.tsx",
  ),
  "utf8",
);
const settingsPageSource = await fs.readFile(
  path.join(appsRoot, "src", "app", "settings", "page.tsx"),
  "utf8",
);
const globalsSource = await fs.readFile(
  path.join(appsRoot, "src", "app", "globals.css"),
  "utf8",
);

test("automatic updater defers network access until startup is idle", () => {
  assert.match(
    checkerSource,
    /AUTO_UPDATE_CHECK_INTERVAL_MS = 7 \* 60 \* 60 \* 1_000/,
  );
  assert.match(checkerSource, /AUTO_UPDATE_INITIAL_DELAY_MS = 5_000/);
  assert.match(checkerSource, /AUTO_UPDATE_IDLE_TIMEOUT_MS = 30_000/);
  assert.match(
    checkerSource,
    /scheduleCheck\(initialAutomaticCheckDelay\(Date\.now\(\)\)\)/,
  );
  assert.match(checkerSource, /window\.requestIdleCallback\(checkWhenIdle/);
  assert.match(
    checkerSource,
    /runCheck\(\)\.finally\([\s\S]*scheduleCheck\(AUTO_UPDATE_CHECK_INTERVAL_MS\)/,
  );
  assert.match(checkerSource, /window\.clearTimeout\(timeoutId\)/);
  assert.match(checkerSource, /window\.cancelIdleCallback\(idleCallbackId\)/);
  assert.doesNotMatch(
    checkerSource,
    /useEffect\(\(\) => \{\s*void runCheck\(\)/,
  );
});

test("automatic updater persists a cooldown across UI reconstruction", () => {
  assert.match(
    checkerSource,
    /codexmanager\.update\.lastAutomaticCheckAt/,
  );
  assert.match(checkerSource, /readLastAutomaticCheckAt\(\)/);
  assert.match(
    checkerSource,
    /const summary = await checkForUpdate\(\);[\s\S]*if \(!activeRef\.current\) return;[\s\S]*recordAutomaticCheckCompleted\(Date\.now\(\)\)/,
  );
  assert.match(
    checkerSource,
    /AUTO_UPDATE_CHECK_INTERVAL_MS - \(now - lastCheckAt\)/,
  );
});

test("automatic updater drops in-flight results after its UI unmounts", () => {
  assert.match(
    checkerSource,
    /await appClient\.showMainWindow\(\)[\s\S]*if \(!activeRef\.current\) return;[\s\S]*recordAutomaticCheckCompleted\(Date\.now\(\)\)[\s\S]*setUpdateCheck\(summary\)/,
  );
  assert.match(
    checkerSource,
    /return \(\) => \{[\s\S]*activeRef\.current = false;[\s\S]*disposed = true/,
  );
  assert.doesNotMatch(
    checkerSource,
    /recordAutomaticCheckCompleted\(Date\.now\(\)\);\s*try/,
  );
});

test("an available update restores and focuses the main window before opening the dialog", () => {
  assert.match(
    checkerSource,
    /if \(!summary\.hasUpdate\) \{[\s\S]*return;[\s\S]*await appClient\.showMainWindow\(\)\.catch\(\(error: unknown\) => \{[\s\S]*reportClientWarning\("update_show_main_window_failed", error\);[\s\S]*\}\);[\s\S]*setDialogOpen\(true\)/,
  );
});

test("automatic updater starts only after desktop settings are ready and enabled", () => {
  assert.match(
    bootstrapSource,
    /!isTrayPreview[\s\S]*!isInitializing[\s\S]*!showCodexGuide[\s\S]*isDesktopRuntime[\s\S]*desktopStartupSettled[\s\S]*appSettings\.updateAutoCheck[\s\S]*<AutomaticUpdateChecker/,
  );
  assert.match(
    bootstrapSource,
    /connectToDesktopService[\s\S]*\.finally\(\(\) => \{[\s\S]*setDesktopStartupSettled\(true\)/,
  );
});

test("automatic updater has no development-only forced dialog path", () => {
  assert.doesNotMatch(checkerSource, /IS_UPDATE_DIALOG_DEMO|9\.9\.9-test/);
  assert.doesNotMatch(bootstrapSource, /NODE_ENV === "development"/);
  assert.match(checkerSource, /const summary = await checkForUpdate\(\)/);
  assert.match(checkerSource, /const summary = await appClient\.prepareUpdate\(\)/);
});

test("basic settings exposes the persisted automatic update toggle", () => {
  assert.match(settingsCardSource, /checked=\{snapshot\.updateAutoCheck\}/);
  assert.match(
    settingsCardSource,
    /updateSettings\.mutate\(\{ updateAutoCheck: value \}\)/,
  );
  assert.match(settingsCardSource, /每 7 小时检查一次/);
});

test("update dialogs use a compact hover treatment for the later button", () => {
  assert.match(
    checkerSource,
    /variant="outline"[\s\S]*className="update-dialog-later-button"[\s\S]*t\("稍后"\)/,
  );
  assert.match(
    settingsPageSource,
    /variant="outline"[\s\S]*className="update-dialog-later-button"[\s\S]*t\("稍后"\)/,
  );
  assert.match(
    globalsSource,
    /update-dialog-later-button:hover[\s\S]*box-shadow: none/,
  );
});
