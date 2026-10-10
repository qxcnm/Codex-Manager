#!/usr/bin/env bash
set -euo pipefail

DB_PATH="${CODEXMANAGER_DB_PATH:-/Users/kilimiao/Codex-Manager/.codex-manager-run/codexmanager-dev.db}"
GATEWAY_URL="${CODEXMANAGER_GATEWAY_URL:-http://localhost:48760}"
QUOTA_LIMIT_TOKENS="${QUOTA_LIMIT_TOKENS:-1000}"

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 1
  fi
}

require_cmd curl
require_cmd node
require_cmd openssl
require_cmd shasum
require_cmd sqlite3

if [[ ! -f "$DB_PATH" ]]; then
  echo "database not found: $DB_PATH" >&2
  exit 1
fi

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/codexmanager-quota.XXXXXX")"
MOCK_PORT_FILE="$TMP_DIR/mock-port"
MOCK_HITS_FILE="$TMP_DIR/mock-hits"
MOCK_LOG_FILE="$TMP_DIR/mock.log"
FIRST_BODY_FILE="$TMP_DIR/first-body.json"
SECOND_BODY_FILE="$TMP_DIR/second-body.json"
MOCK_PID=""
KEY_ID=""
AGGREGATE_API_ID=""

cleanup() {
  local status=$?
  if [[ -n "${MOCK_PID:-}" ]]; then
    kill "$MOCK_PID" >/dev/null 2>&1 || true
    wait "$MOCK_PID" >/dev/null 2>&1 || true
  fi
  if [[ -n "${KEY_ID:-}" || -n "${AGGREGATE_API_ID:-}" ]]; then
    sqlite3 "$DB_PATH" <<SQL >/dev/null 2>&1 || true
BEGIN;
DELETE FROM request_token_stats WHERE key_id = '$KEY_ID';
DELETE FROM request_logs WHERE key_id = '$KEY_ID';
DELETE FROM api_key_quota_limits WHERE key_id = '$KEY_ID';
DELETE FROM api_key_secrets WHERE key_id = '$KEY_ID';
DELETE FROM api_key_profiles WHERE key_id = '$KEY_ID';
DELETE FROM api_keys WHERE id = '$KEY_ID';
DELETE FROM aggregate_api_secrets WHERE aggregate_api_id = '$AGGREGATE_API_ID';
DELETE FROM aggregate_apis WHERE id = '$AGGREGATE_API_ID';
COMMIT;
SQL
  fi
  rm -rf "$TMP_DIR"
  exit "$status"
}
trap cleanup EXIT

MOCK_HITS_FILE="$MOCK_HITS_FILE" MOCK_PORT_FILE="$MOCK_PORT_FILE" \
  node <<'NODE' >"$MOCK_LOG_FILE" 2>&1 &
const fs = require("node:fs");
const http = require("node:http");

const hitsFile = process.env.MOCK_HITS_FILE;
const portFile = process.env.MOCK_PORT_FILE;
let hits = 0;

const server = http.createServer((req, res) => {
  const chunks = [];
  req.on("data", (chunk) => chunks.push(chunk));
  req.on("end", () => {
    hits += 1;
    fs.writeFileSync(hitsFile, String(hits));
    res.writeHead(200, {
      "content-type": "application/json",
      "x-mock-upstream": "codexmanager-quota",
    });
    res.end(JSON.stringify({
      id: "chatcmpl_quota_mock",
      object: "chat.completion",
      created: Math.floor(Date.now() / 1000),
      model: "gpt-6-luna",
      choices: [{
        index: 0,
        message: { role: "assistant", content: "quota mock ok" },
        finish_reason: "stop",
      }],
      usage: {
        prompt_tokens: 700,
        completion_tokens: 300,
        total_tokens: 1000,
      },
    }));
  });
});

server.listen(0, "127.0.0.1", () => {
  fs.writeFileSync(portFile, String(server.address().port));
});

process.on("SIGTERM", () => {
  server.close(() => process.exit(0));
});
NODE
MOCK_PID=$!

for _ in {1..50}; do
  if [[ -s "$MOCK_PORT_FILE" ]]; then
    break
  fi
  sleep 0.1
done

if [[ ! -s "$MOCK_PORT_FILE" ]]; then
  echo "mock upstream did not start" >&2
  cat "$MOCK_LOG_FILE" >&2 || true
  exit 1
fi

MOCK_PORT="$(cat "$MOCK_PORT_FILE")"
MOCK_URL="http://127.0.0.1:${MOCK_PORT}"
STAMP="$(date +%s)"
KEY_ID="gk_quota_mock_${STAMP}"
AGGREGATE_API_ID="ag_quota_mock_${STAMP}"
PLATFORM_KEY="$(openssl rand -hex 32)"
PLATFORM_KEY_HASH="$(printf "%s" "$PLATFORM_KEY" | shasum -a 256 | awk '{print $1}')"
NOW="$(date +%s)"

sqlite3 "$DB_PATH" <<SQL
BEGIN;
INSERT INTO aggregate_apis (
  id, provider_type, supplier_name, sort, url, status,
  created_at, updated_at, last_test_at, last_test_status, last_test_error,
  auth_type, auth_params_json, action
) VALUES (
  '$AGGREGATE_API_ID', 'codex', 'quota mock upstream', -999, '$MOCK_URL', 'active',
  $NOW, $NOW, NULL, NULL, NULL,
  'apikey', NULL, NULL
);
INSERT INTO aggregate_api_secrets (aggregate_api_id, secret_value, created_at, updated_at)
VALUES ('$AGGREGATE_API_ID', 'mock-upstream-key', $NOW, $NOW);
INSERT INTO api_keys (
  id, name, key_hash, status, created_at, last_used_at,
  model_slug, reasoning_effort, rotation_strategy, aggregate_api_id, account_plan_filter
) VALUES (
  '$KEY_ID', 'quota limit mock verification', '$PLATFORM_KEY_HASH', 'active', $NOW, NULL,
  NULL, NULL, 'aggregate_api_rotation', '$AGGREGATE_API_ID', NULL
);
INSERT INTO api_key_profiles (
  key_id, client_type, protocol_type, auth_scheme, upstream_base_url,
  static_headers_json, default_model, reasoning_effort, service_tier, created_at, updated_at
) VALUES (
  '$KEY_ID', 'codex', 'openai_compat', 'authorization_bearer', NULL,
  NULL, NULL, NULL, NULL, $NOW, $NOW
);
INSERT INTO api_key_secrets (key_id, key_value, created_at, updated_at)
VALUES ('$KEY_ID', '$PLATFORM_KEY', $NOW, $NOW);
INSERT INTO api_key_quota_limits (key_id, quota_limit_tokens, created_at, updated_at)
VALUES ('$KEY_ID', $QUOTA_LIMIT_TOKENS, $NOW, $NOW);
COMMIT;
SQL

echo "created temporary platform key: $KEY_ID"
echo "mock upstream: $MOCK_URL"
echo "quota limit: ${QUOTA_LIMIT_TOKENS} tokens"

FIRST_STATUS="$(curl -sS -o "$FIRST_BODY_FILE" -w "%{http_code}" --max-time 10 \
  -X POST "${GATEWAY_URL}/v1/chat/completions" \
  -H "Authorization: Bearer ${PLATFORM_KEY}" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-6-luna","messages":[{"role":"user","content":"hello"}]}')"

if [[ "$FIRST_STATUS" != "200" ]]; then
  echo "first request expected 200, got $FIRST_STATUS" >&2
  cat "$FIRST_BODY_FILE" >&2 || true
  exit 1
fi

USED_TOKENS="0"
for _ in {1..30}; do
  USED_TOKENS="$(sqlite3 "$DB_PATH" "SELECT COALESCE(SUM(CASE WHEN total_tokens IS NOT NULL THEN total_tokens ELSE COALESCE(input_tokens,0) - COALESCE(cached_input_tokens,0) + COALESCE(output_tokens,0) END),0) FROM request_token_stats WHERE key_id = '$KEY_ID';")"
  if [[ "$USED_TOKENS" -ge "$QUOTA_LIMIT_TOKENS" ]]; then
    break
  fi
  sleep 0.2
done

if [[ "$USED_TOKENS" -lt "$QUOTA_LIMIT_TOKENS" ]]; then
  echo "usage was not recorded up to the quota limit; used=$USED_TOKENS" >&2
  cat "$FIRST_BODY_FILE" >&2 || true
  exit 1
fi

HITS_AFTER_FIRST="$(cat "$MOCK_HITS_FILE")"

SECOND_STATUS="$(curl -sS -o "$SECOND_BODY_FILE" -w "%{http_code}" --max-time 10 \
  -X POST "${GATEWAY_URL}/v1/chat/completions" \
  -H "Authorization: Bearer ${PLATFORM_KEY}" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-6-luna","messages":[{"role":"user","content":"hello again"}]}')"

SECOND_BODY="$(cat "$SECOND_BODY_FILE")"
HITS_AFTER_SECOND="$(cat "$MOCK_HITS_FILE")"

if [[ "$SECOND_STATUS" != "429" ]]; then
  echo "second request expected 429, got $SECOND_STATUS" >&2
  echo "$SECOND_BODY" >&2
  exit 1
fi

if [[ "$SECOND_BODY" != *"quota exhausted"* && "$SECOND_BODY" != *"额度已用尽"* ]]; then
  echo "second request did not return quota exhaustion message" >&2
  echo "$SECOND_BODY" >&2
  exit 1
fi

if [[ "$HITS_AFTER_SECOND" != "$HITS_AFTER_FIRST" ]]; then
  echo "mock upstream was hit after quota exhaustion; before=$HITS_AFTER_FIRST after=$HITS_AFTER_SECOND" >&2
  exit 1
fi

echo "PASS first request reached mock upstream: status=$FIRST_STATUS, mock_hits=$HITS_AFTER_FIRST"
echo "PASS gateway recorded usage: used_tokens=$USED_TOKENS"
echo "PASS second request blocked before upstream: status=$SECOND_STATUS, mock_hits=$HITS_AFTER_SECOND"
