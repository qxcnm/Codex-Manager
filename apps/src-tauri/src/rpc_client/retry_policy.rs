use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum RpcReplaySafety {
    ReadOnly,
    IdempotentWrite,
    NonIdempotentWrite,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RpcRetryPolicy {
    read_only_methods: Vec<String>,
    idempotent_write_methods: Vec<String>,
}

// The Web and desktop transports deliberately share the same exact allowlist.
// A new or unreviewed RPC must never become replayable merely because of its name.
pub(super) fn rpc_replay_safety(method: &str) -> RpcReplaySafety {
    static POLICY: OnceLock<RpcRetryPolicy> = OnceLock::new();
    let policy = POLICY.get_or_init(|| {
        serde_json::from_str(include_str!("../../../src/lib/api/rpc-retry-policy.json"))
            .expect("bundled RPC retry policy must be valid")
    });
    if policy.read_only_methods.iter().any(|known| known == method) {
        RpcReplaySafety::ReadOnly
    } else if policy
        .idempotent_write_methods
        .iter()
        .any(|known| known == method)
    {
        RpcReplaySafety::IdempotentWrite
    } else {
        RpcReplaySafety::NonIdempotentWrite
    }
}

#[cfg(test)]
mod tests {
    use super::{rpc_replay_safety, RpcReplaySafety};

    #[test]
    fn shared_rpc_policy_is_conservative_and_explicit() {
        assert_eq!(rpc_replay_safety("account/list"), RpcReplaySafety::ReadOnly);
        assert_eq!(
            rpc_replay_safety("modelDiscovery/list"),
            RpcReplaySafety::ReadOnly
        );
        assert_eq!(
            rpc_replay_safety("apikey/disable"),
            RpcReplaySafety::IdempotentWrite
        );
        for method in [
            "apikey/create",
            "modelDiscovery/update",
            "account/usage/resetCredit/consume",
            "accountManager/wallet/topUp",
            "appSettings/get",
            "authorContent/get",
            "future/get",
            "future/set",
        ] {
            assert_eq!(
                rpc_replay_safety(method),
                RpcReplaySafety::NonIdempotentWrite,
                "unreviewed or non-idempotent method must not replay: {method}"
            );
        }
    }
}
