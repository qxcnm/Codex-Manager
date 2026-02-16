import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const appsRoot = path.resolve(__dirname, '..');

const readText = (filePath) => fs.readFileSync(filePath, 'utf8');
const readJson = (filePath) => JSON.parse(readText(filePath));

test('apps layout and branding updated to CodexManager', () => {
  const tauriPath = path.join(appsRoot, 'src-tauri', 'tauri.conf.json');
  assert.ok(fs.existsSync(tauriPath), `missing ${tauriPath}`);

  const tauri = readJson(tauriPath);
  assert.equal(tauri.productName, 'CodexManager');
  assert.equal(tauri?.app?.windows?.[0]?.title, 'CodexManager');

  const indexPath = path.join(appsRoot, 'index.html');
  assert.ok(fs.existsSync(indexPath), `missing ${indexPath}`);
  const indexHtml = readText(indexPath);
  assert.ok(indexHtml.includes('<title>CodexManager</title>'), 'index title not updated');
  assert.ok(indexHtml.includes('<h1>CodexManager</h1>'), 'index brand not updated');

  const distPath = path.join(appsRoot, 'dist', 'index.html');
  if (fs.existsSync(distPath)) {
    const distHtml = readText(distPath);
    assert.ok(distHtml.includes('<title>CodexManager</title>'), 'dist title not updated');
    assert.ok(distHtml.includes('<h1>CodexManager</h1>'), 'dist brand not updated');
  }
});
