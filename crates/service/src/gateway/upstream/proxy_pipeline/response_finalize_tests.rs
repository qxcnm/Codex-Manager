use super::{derive_final_error, derive_status_for_log, is_client_disconnect_error};

#[test]
fn derive_final_error_prefers_upstream_hint_then_http_error_then_bridge_error() {
    assert_eq!(
        derive_final_error(
            429,
            Some("last attempt"),
            Some("upstream hint"),
            Some("bridge error".to_string()),
        )
        .as_deref(),
        Some("upstream hint")
    );
    assert_eq!(
        derive_final_error(
            429,
            Some("last attempt"),
            None,
            Some("bridge error".to_string())
        )
        .as_deref(),
        Some("last attempt")
    );
    assert_eq!(
        derive_final_error(200, None, None, Some("bridge error".to_string())).as_deref(),
        Some("bridge error")
    );
}

#[test]
fn derive_status_for_log_respects_disconnect_delivery_and_bridge_fallbacks() {
    assert_eq!(
        derive_status_for_log(200, None, true, false, false, true),
        499
    );
    assert_eq!(
        derive_status_for_log(200, Some(207), true, false, false, false),
        207
    );
    assert_eq!(
        derive_status_for_log(404, None, true, false, false, false),
        404
    );
    assert_eq!(
        derive_status_for_log(200, None, true, true, false, false),
        502
    );
    assert_eq!(
        derive_status_for_log(200, None, false, false, false, false),
        502
    );
    assert_eq!(
        derive_status_for_log(200, None, true, false, false, false),
        200
    );
}

#[test]
fn client_disconnect_error_matches_common_socket_messages() {
    assert!(is_client_disconnect_error("broken pipe"));
    assert!(is_client_disconnect_error("connection reset by peer"));
    assert!(is_client_disconnect_error(
        "你的主机中的软件中止了一个已建立的连接。 (os error 10053)"
    ));
    assert!(!is_client_disconnect_error("upstream timeout"));
}
