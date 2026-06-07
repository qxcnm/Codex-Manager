import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import ts from "../node_modules/typescript/lib/typescript.js";

const appsRoot = path.resolve(import.meta.dirname, "..");

const SOURCE_SKIP_DIRS = new Set([".next", "node_modules", "out"]);
const CJK_TEXT_PATTERN = /[\u4e00-\u9fff]/;

async function readSource(relativePath) {
  return fs.readFile(path.join(appsRoot, relativePath), "utf8");
}

async function collectSourceFiles(relativeDir, files = []) {
  const absoluteDir = path.join(appsRoot, relativeDir);
  const entries = await fs.readdir(absoluteDir, { withFileTypes: true });
  for (const entry of entries) {
    const relativePath = path.join(relativeDir, entry.name);
    if (entry.isDirectory()) {
      if (!SOURCE_SKIP_DIRS.has(entry.name)) {
        await collectSourceFiles(relativePath, files);
      }
      continue;
    }
    if (
      /\.(ts|tsx)$/.test(entry.name) &&
      !relativePath.includes(path.join("src", "lib", "i18n", "messages"))
    ) {
      files.push(relativePath);
    }
  }
  return files;
}

function parseSource(relativePath, source) {
  return ts.createSourceFile(
    relativePath,
    source,
    ts.ScriptTarget.Latest,
    true,
    relativePath.endsWith(".tsx") ? ts.ScriptKind.TSX : ts.ScriptKind.TS,
  );
}

function readPropertyName(name) {
  if (ts.isStringLiteral(name) || ts.isNumericLiteral(name)) return String(name.text);
  if (ts.isIdentifier(name)) return name.text;
  return null;
}

function extractStaticTKeys(relativePath, source) {
  const keys = [];
  const sourceFile = parseSource(relativePath, source);
  function visit(node) {
    if (ts.isCallExpression(node) && ts.isIdentifier(node.expression) && node.expression.text === "t") {
      const firstArg = node.arguments[0];
      if (firstArg && ts.isStringLiteral(firstArg)) {
        keys.push(firstArg.text);
      }
    }
    ts.forEachChild(node, visit);
  }
  visit(sourceFile);
  return keys;
}

function extractMessageKeys(relativePath, source) {
  const keys = [];
  const sourceFile = parseSource(relativePath, source);
  function visit(node) {
    if (ts.isPropertyAssignment(node)) {
      const key = readPropertyName(node.name);
      if (key) keys.push(key);
    }
    ts.forEachChild(node, visit);
  }
  visit(sourceFile);
  return keys;
}

function isStaticTCall(node) {
  return (
    ts.isCallExpression(node) &&
    ts.isIdentifier(node.expression) &&
    node.expression.text === "t" &&
    node.arguments[0] &&
    ts.isStringLiteral(node.arguments[0])
  );
}

function isInsideStaticTCall(node) {
  let current = node;
  while (current) {
    if (isStaticTCall(current)) return true;
    current = current.parent;
  }
  return false;
}

function isInsideImportOrType(node) {
  let current = node;
  while (current) {
    if (
      ts.isImportDeclaration(current) ||
      ts.isTypeNode(current) ||
      ts.isInterfaceDeclaration(current) ||
      ts.isTypeAliasDeclaration(current)
    ) {
      return true;
    }
    current = current.parent;
  }
  return false;
}

function sourceLocation(sourceFile, node) {
  const position = sourceFile.getLineAndCharacterOfPosition(node.getStart(sourceFile));
  return `${position.line + 1}:${position.character + 1}`;
}

function extractChineseStringLiteralKeys(relativePath, source) {
  const entries = [];
  const sourceFile = parseSource(relativePath, source);

  function visit(node) {
    if (
      ts.isStringLiteral(node) &&
      CJK_TEXT_PATTERN.test(node.text) &&
      !isInsideImportOrType(node)
    ) {
      entries.push({
        file: relativePath,
        loc: sourceLocation(sourceFile, node),
        key: node.text.trim(),
      });
    }
    ts.forEachChild(node, visit);
  }

  visit(sourceFile);
  return entries;
}

function extractLiteralChineseUiLeaks(relativePath, source) {
  const leaks = [];
  const sourceFile = parseSource(relativePath, source);

  function visit(node) {
    if (ts.isJsxText(node)) {
      const text = node.getText(sourceFile).replace(/[{}\s]/g, "");
      if (CJK_TEXT_PATTERN.test(text)) {
        leaks.push({
          file: relativePath,
          loc: sourceLocation(sourceFile, node),
          kind: "jsxText",
          text: node.getText(sourceFile).trim(),
        });
      }
    }

    if (ts.isJsxAttribute(node) && node.initializer) {
      const initializer = node.initializer;
      if (ts.isStringLiteral(initializer) && CJK_TEXT_PATTERN.test(initializer.text)) {
        leaks.push({
          file: relativePath,
          loc: sourceLocation(sourceFile, node),
          kind: `attr:${node.name.getText(sourceFile)}`,
          text: initializer.text,
        });
      }
      if (
        ts.isJsxExpression(initializer) &&
        initializer.expression &&
        ts.isStringLiteral(initializer.expression) &&
        CJK_TEXT_PATTERN.test(initializer.expression.text) &&
        !isInsideStaticTCall(initializer.expression)
      ) {
        leaks.push({
          file: relativePath,
          loc: sourceLocation(sourceFile, node),
          kind: `attrExpr:${node.name.getText(sourceFile)}`,
          text: initializer.expression.text,
        });
      }
    }

    ts.forEachChild(node, visit);
  }

  visit(sourceFile);
  return leaks;
}

async function collectUsedKeysByFile(files) {
  const entries = await Promise.all(
    files.map(async (file) => [file, extractStaticTKeys(file, await readSource(file))]),
  );
  return new Map(entries.filter(([, keys]) => keys.length > 0));
}

async function collectLocaleKeys(locale) {
  const messagesDir = path.join(appsRoot, "src", "lib", "i18n", "messages");
  const sectionDir = path.join(messagesDir, "sections");
  const sectionFiles = (await fs.readdir(sectionDir))
    .filter((file) => file.startsWith(`${locale}-`) && file.endsWith(".ts"))
    .map((file) => `src/lib/i18n/messages/sections/${file}`);
  const files = [`src/lib/i18n/messages/${locale}.ts`, ...sectionFiles];

  return new Set(
    (
      await Promise.all(
        files.map(async (file) => extractMessageKeys(file, await readSource(file))),
      )
    ).flat(),
  );
}

test("src 静态 t() 文案都有非中文翻译", async () => {
  const sourceFiles = await collectSourceFiles("src");
  assert.ok(sourceFiles.length > 0, "未读取到 src 源文件");

  const usedKeysByFile = await collectUsedKeysByFile(sourceFiles);
  assert.ok(usedKeysByFile.size > 0, "未读取到静态 t() 文案");

  const localeKeys = new Map(
    await Promise.all(
      ["en", "ko", "ru"].map(async (locale) => [locale, await collectLocaleKeys(locale)]),
    ),
  );

  for (const [locale, keys] of localeKeys) {
    const missingByFile = [];
    for (const [file, usedKeys] of usedKeysByFile) {
      const missing = [...new Set(usedKeys)].filter((key) => !keys.has(key)).sort();
      if (missing.length > 0) {
        missingByFile.push({ file, missing });
      }
    }
    assert.deepEqual(missingByFile, [], `${locale} 缺少静态翻译`);
  }
});

test("源码中文字符串 key 都有非中文翻译", async () => {
  const sourceFiles = await collectSourceFiles("src");
  const localeKeys = new Map(
    await Promise.all(
      ["en", "ko", "ru"].map(async (locale) => [locale, await collectLocaleKeys(locale)]),
    ),
  );
  const keyEntries = (
    await Promise.all(
      sourceFiles.map(async (file) =>
        extractChineseStringLiteralKeys(file, await readSource(file)),
      ),
    )
  ).flat();
  const keys = new Map();
  for (const entry of keyEntries) {
    if (!entry.key) continue;
    if (!keys.has(entry.key)) keys.set(entry.key, []);
    keys.get(entry.key).push(`${entry.file}:${entry.loc}`);
  }

  for (const [locale, knownKeys] of localeKeys) {
    const missing = [...keys.entries()]
      .filter(([key]) => !knownKeys.has(key))
      .map(([key, sites]) => ({ key, sites: sites.slice(0, 4) }))
      .sort((a, b) => a.key.localeCompare(b.key, "zh-Hans-CN"));
    assert.deepEqual(missing, [], `${locale} 缺少源码中文字符串翻译`);
  }
});

test("JSX 直接展示的中文文案必须走 t()", async () => {
  const sourceFiles = await collectSourceFiles("src");
  const leaks = (
    await Promise.all(
      sourceFiles.map(async (file) =>
        extractLiteralChineseUiLeaks(file, await readSource(file)),
      ),
    )
  ).flat();

  assert.deepEqual(leaks, []);
});
