import { readdirSync } from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";

function collectTestFiles(rootDir, suffix, out) {
  const entries = readdirSync(rootDir, { withFileTypes: true });
  for (const entry of entries) {
    const fullPath = path.join(rootDir, entry.name);
    if (entry.isDirectory()) {
      collectTestFiles(fullPath, suffix, out);
      continue;
    }
    if (entry.isFile() && entry.name.endsWith(suffix)) {
      out.push(fullPath);
    }
  }
}

const [targetDir, suffix] = process.argv.slice(2);
if (!targetDir || !suffix) {
  console.error("Usage: node scripts/run-tests.mjs <target-dir> <file-suffix>");
  process.exit(1);
}

const rootDir = path.resolve(process.cwd(), targetDir);
const files = [];
collectTestFiles(rootDir, suffix, files);
files.sort();

if (files.length === 0) {
  console.error(`No test files found in ${targetDir} with suffix ${suffix}`);
  process.exit(1);
}

const result = spawnSync(process.execPath, ["--test", ...files], {
  stdio: "inherit",
});

process.exit(result.status ?? 1);
