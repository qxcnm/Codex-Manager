//! Short-lived recovery for a rejected previous id on a *different* native socket.
//! Never index history by model, a caller-supplied call id, or a prompt cache key.
use super::super::{PreparedClientFrame, WsRequestContext};
use super::{ws_tool_output_key, CompletedWsResponseCache, Value, WsToolCallKey, WsToolCallKind};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

const TTL: Duration = Duration::from_secs(30 * 60);
const MAX_RECEIPTS: usize = 64;
const MAX_ENTRY_BYTES: usize = 2 * 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in super::super) struct ToolReceiptScope([u8; 32]);

impl ToolReceiptScope {
    pub(in super::super) fn for_request(
        ctx: &WsRequestContext,
        frame: &PreparedClientFrame,
        account_id: &str,
    ) -> Option<Self> {
        let session = ctx
            .cache_affinity_key
            .as_deref()
            .or(frame.tool_call_registry_session_id.as_deref())?;
        let fields = [
            ctx.api_key.key_hash.as_str(),
            ctx.api_key.protocol_type.as_str(),
            ctx.effective_upstream_base.as_str(),
            account_id,
            session,
        ];
        let mut digest = Sha256::new();
        for field in fields {
            if field.trim().is_empty() {
                return None;
            }
            digest.update((field.len() as u64).to_be_bytes());
            digest.update(field.as_bytes());
        }
        Some(Self(digest.finalize().into()))
    }
}

struct Receipt {
    scope: ToolReceiptScope,
    id: String,
    calls: Vec<WsToolCallKey>,
    history: Arc<[u8]>,
    bytes: usize,
    created: Instant,
}

#[derive(Default)]
struct Receipts(VecDeque<Receipt>);
static RECEIPTS: LazyLock<Mutex<Receipts>> = LazyLock::new(Default::default);

impl Receipts {
    fn prune(&mut self, now: Instant) {
        self.0
            .retain(|r| now.saturating_duration_since(r.created) < TTL);
    }
    fn insert(&mut self, receipt: Receipt) {
        self.prune(receipt.created);
        self.0
            .retain(|r| !(r.scope == receipt.scope && r.id == receipt.id));
        if receipt.bytes > MAX_ENTRY_BYTES {
            return;
        }
        let mut bytes: usize = self.0.iter().map(|r| r.bytes).sum();
        while self.0.len() >= MAX_RECEIPTS || bytes + receipt.bytes > MAX_TOTAL_BYTES {
            let Some(old) = self.0.pop_front() else {
                return;
            };
            bytes -= old.bytes;
        }
        self.0.push_back(receipt);
    }
    fn find(
        &mut self,
        scope: ToolReceiptScope,
        id: &str,
        outputs: &[WsToolCallKey],
        now: Instant,
    ) -> Option<Arc<[u8]>> {
        self.prune(now);
        self.0
            .iter()
            .find(|r| {
                r.scope == scope
                    && r.id == id
                    && !outputs.is_empty()
                    && outputs.iter().all(|key| r.calls.contains(key))
            })
            .map(|r| r.history.clone())
    }
}

fn completed_tool(item: &Value) -> Option<WsToolCallKey> {
    if !item.get("status").is_none_or(|s| s == "completed") {
        return None;
    }
    let kind = match item.get("type")?.as_str()? {
        "function_call" => WsToolCallKind::Function,
        "custom_tool_call" => WsToolCallKind::Custom,
        "tool_search_call" if item.get("execution").and_then(Value::as_str) == Some("client") => {
            WsToolCallKind::ToolSearch
        }
        _ => return None,
    };
    let id = item.get("call_id")?.as_str()?.trim();
    (!id.is_empty()).then(|| WsToolCallKey {
        kind,
        call_id: id.to_owned(),
    })
}

impl CompletedWsResponseCache {
    pub(super) fn publish_pending_receipt(&self, scope: ToolReceiptScope, id: &str) {
        let Some(last) = self.responses.get(id) else {
            return;
        };
        if last.retained_bytes > MAX_ENTRY_BYTES {
            return;
        }
        let calls: Vec<_> = last
            .output
            .iter()
            .filter_map(completed_tool)
            .take(257)
            .collect();
        if calls.is_empty() || calls.len() > 256 {
            return;
        }
        let Ok(chain) = self.history_chain(id) else {
            return;
        };
        if chain.iter().any(|r| r.receipt_scope != Some(scope))
            || chain.iter().map(|r| r.retained_bytes).sum::<usize>() > MAX_ENTRY_BYTES
        {
            return;
        }
        // Serialize references, not a clone of the connection's entire cache.
        let items: Vec<_> = chain
            .into_iter()
            .rev()
            .flat_map(|r| r.input.iter().chain(&r.output))
            .collect();
        let Ok(history) = serde_json::to_vec(&items) else {
            return;
        };
        let bytes = history.len()
            + id.len()
            + calls
                .iter()
                .map(|k| k.call_id.len() + std::mem::size_of::<WsToolCallKey>())
                .sum::<usize>();
        if bytes > MAX_ENTRY_BYTES {
            return;
        }
        let history = history.into();
        if let Ok(mut receipts) = RECEIPTS.lock() {
            receipts.insert(Receipt {
                scope,
                id: id.to_owned(),
                calls,
                history,
                bytes,
                created: Instant::now(),
            });
        }
    }

    pub(in super::super) fn expand_pending_receipt(
        scope: ToolReceiptScope,
        frame: &PreparedClientFrame,
    ) -> Option<String> {
        let id = frame.previous_response_id.as_deref()?;
        let input = match &frame.input {
            Value::Array(items) => items.as_slice(),
            item => std::slice::from_ref(item),
        };
        let mut outputs = Vec::new();
        for item in input {
            if let Some(key) = ws_tool_output_key(item).ok()? {
                if outputs.len() >= 256 {
                    return None;
                }
                outputs.push(key);
            } else if item.get("role").and_then(Value::as_str) != Some("user")
                || !item.get("type").is_none_or(|t| t == "message")
            {
                // Explicit history, foreign references or unknown input are not a receipt-only continuation.
                return None;
            }
        }
        let history = RECEIPTS
            .lock()
            .ok()?
            .find(scope, id, &outputs, Instant::now())?;
        let mut items: Vec<Value> = serde_json::from_slice(&history).ok()?;
        let mut request: Value = serde_json::from_str(&frame.text).ok()?;
        let object = request.as_object_mut()?;
        items.extend(input.iter().cloned());
        object.remove("previous_response_id");
        object.insert("input".into(), items.into());
        serde_json::to_string(&request).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn websocket_receipts_expire_and_obey_count_and_byte_budgets() {
        let mut cache = Receipts::default();
        let now = Instant::now();
        let scope = ToolReceiptScope([0; 32]);
        let key = WsToolCallKey {
            kind: WsToolCallKind::Custom,
            call_id: "observed".into(),
        };
        let entry = |i: usize, size| Receipt {
            scope,
            id: i.to_string(),
            calls: vec![key.clone()],
            history: Arc::from(&b"[]"[..]),
            bytes: size,
            created: now,
        };
        for i in 0..=MAX_RECEIPTS {
            cache.insert(entry(i, 50));
        }
        assert_eq!(cache.0.len(), MAX_RECEIPTS);
        assert!(cache
            .find(scope, "0", std::slice::from_ref(&key), now)
            .is_none());
        assert!(cache
            .find(scope, "1", std::slice::from_ref(&key), now)
            .is_some());
        assert!(cache
            .find(scope, "1", std::slice::from_ref(&key), now + TTL)
            .is_none());
        assert!(cache.0.is_empty());
        for i in 0..=MAX_TOTAL_BYTES / MAX_ENTRY_BYTES {
            cache.insert(entry(i, MAX_ENTRY_BYTES));
        }
        assert_eq!(cache.0.len(), MAX_TOTAL_BYTES / MAX_ENTRY_BYTES);
        cache.insert(entry(100, MAX_ENTRY_BYTES + 1));
        assert!(cache
            .find(scope, "100", std::slice::from_ref(&key), now)
            .is_none());
    }
}
