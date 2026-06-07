import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";

const appsRoot = path.resolve(import.meta.dirname, "..");

async function readSource(relativePath) {
  return fs.readFile(path.join(appsRoot, relativePath), "utf8");
}

function readConstFunctionBody(source, functionName) {
  const start = source.indexOf(`const ${functionName} = async () => {`);
  assert.notEqual(start, -1, `${functionName} not found`);
  const end = source.indexOf("\n  };\n", start);
  assert.notEqual(end, -1, `${functionName} body end not found`);
  return source.slice(start, end);
}

test("账号实体列表不会被用量刷新路径自动打空", async () => {
  const source = await readSource("src/hooks/useAccounts.ts");
  const invalidateUsageBody = readConstFunctionBody(source, "invalidateUsageData");

  assert.match(
    source,
    /queryKey:\s*\[\s*"accounts",\s*"list"\s*\][\s\S]*staleTime:\s*Infinity/,
  );
  assert.doesNotMatch(invalidateUsageBody, /queryKey:\s*\[\s*"accounts"/);
  assert.match(
    source,
    /const refreshAccountMutation = useMutation\(\{[\s\S]*onSettled:\s*async \(\) => \{[\s\S]*await invalidateUsageData\(\);/,
  );
  assert.match(
    source,
    /const refreshAllMutation = useMutation\(\{[\s\S]*onSettled:\s*async \(\) => \{[\s\S]*await invalidateUsageData\(\);/,
  );
});

test("账号页用启动快照作为账号实体列表的非空初始来源", async () => {
  const source = await readSource("src/hooks/useAccounts.ts");

  assert.match(source, /const startupSnapshotQuery = useQuery\(\{/);
  assert.match(source, /buildAccountListResultFromSnapshot\(startupAccounts\)/);
  assert.match(
    source,
    /account\/list returned empty while startup snapshot still has accounts/,
  );
  assert.match(source, /initialData:\s*\(\) =>[\s\S]*startupAccountList/);
});
