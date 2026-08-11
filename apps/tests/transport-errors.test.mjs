import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";
import ts from "../node_modules/typescript/lib/typescript.js";

const appsRoot = path.resolve(import.meta.dirname, "..");
const sourcePath = path.join(
  appsRoot,
  "src",
  "lib",
  "api",
  "transport-errors.ts"
);

async function loadTransportErrorsModule() {
  const source = await fs.readFile(sourcePath, "utf8");
  const compiled = ts.transpileModule(source, {
    compilerOptions: {
      module: ts.ModuleKind.ES2022,
      target: ts.ScriptTarget.ES2022,
    },
    fileName: sourcePath,
  });

  const tempDir = await fs.mkdtemp(
    path.join(os.tmpdir(), "codexmanager-transport-errors-")
  );
  const tempFile = path.join(tempDir, "transport-errors.mjs");
  await fs.writeFile(tempFile, compiled.outputText, "utf8");
  return import(pathToFileURL(tempFile).href);
}

const transportErrors = await loadTransportErrorsModule();

test("unwrapRpcPayload 统一解包 result 与 error envelope", () => {
  assert.equal(
    transportErrors.unwrapRpcPayload({ result: { ok: true, value: 42 } }).value,
    42
  );
  assert.throws(
    () => transportErrors.unwrapRpcPayload({ error: { message: "boom" } }),
    /boom/
  );
});

test("unwrapRpcPayload 会把 business error 直接提升为异常", () => {
  assert.throws(
    () =>
      transportErrors.unwrapRpcPayload({
        result: { ok: false, error: { message: "业务失败" } },
      }),
    /业务失败/
  );
});

test("getAppErrorMessage 与 isCommandMissingError 复用统一文案规则", () => {
  assert.equal(
    transportErrors.getAppErrorMessage({ ok: false, error: { message: "拒绝访问" } }),
    "拒绝访问"
  );
  assert.equal(
    transportErrors.getAppErrorMessage(new Error("unknown command: demo")),
    "unknown command: demo"
  );
  assert.equal(
    transportErrors.isCommandMissingError(new Error("service_demo is not a registered command")),
    true
  );
});

test("getAppErrorMessage 会把底层 body error 映射为更易懂的上游错误", () => {
  assert.equal(
    transportErrors.getAppErrorMessage(new Error("request or response body error")),
    "上游中途断开，未返回具体错误信息"
  );
  assert.equal(
    transportErrors.getAppErrorMessage("stream read failed"),
    "上游中途断开，未返回具体错误信息"
  );
});

test("getAppErrorMessage 会把流式空闲超时和不完整终态统一收敛", () => {
  assert.equal(
    transportErrors.getAppErrorMessage("stream idle timeout"),
    "上游流式空闲超时"
  );
  assert.equal(
    transportErrors.getAppErrorMessage("code=stream_timeout stream timeout at upstream"),
    "上游流式空闲超时"
  );
  assert.equal(
    transportErrors.getAppErrorMessage("response.incomplete"),
    "连接中断（可能是网络波动或客户端主动取消）"
  );
});

test("getAppErrorMessage 会把旧的网络抖动文案统一收敛为连接中断提示", () => {
  assert.equal(
    transportErrors.getAppErrorMessage("网络抖动"),
    "连接中断（可能是网络波动或客户端主动取消）"
  );
  assert.equal(
    transportErrors.getAppErrorMessage("stream disconnected before completion"),
    "连接中断（可能是网络波动或客户端主动取消）"
  );
});

test("getAppErrorMessage 将 Grok 探测错误转换成可操作提示", () => {
  assert.equal(
    transportErrors.getAppErrorMessage("grok_credential_unauthorized"),
    "Grok 登录凭据已失效，请重新导入",
  );
  assert.equal(
    transportErrors.getAppErrorMessage("grok_antibot_challenge"),
    "Grok 风控验证未通过，请检查固定代理出口后稍后重试",
  );
  assert.equal(
    transportErrors.getAppErrorMessage("grok_quota_shape_unknown"),
    "暂时无法确认该 Grok 账号的套餐等级，未展示未经验证的模型",
  );
});
