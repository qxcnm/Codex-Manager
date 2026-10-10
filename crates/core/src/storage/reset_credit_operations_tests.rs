use std::sync::{Arc, Barrier};

use super::*;

fn storage() -> Storage {
    let storage = Storage::open_in_memory().expect("open in-memory storage");
    storage.init().expect("initialize storage");
    storage
}

#[test]
fn claim_is_idempotent_and_detects_cross_account_reuse() {
    let storage = storage();
    let created = storage
        .claim_reset_credit_operation("operation-1", "account-1", "redeem-1", 10)
        .expect("claim operation");
    assert!(matches!(created, ResetCreditOperationClaim::Created(_)));
    assert_eq!(
        created.operation().status,
        ResetCreditOperationStatus::Pending
    );

    let existing = storage
        .claim_reset_credit_operation("operation-1", "account-1", "redeem-ignored", 11)
        .expect("read existing operation");
    assert!(matches!(existing, ResetCreditOperationClaim::Existing(_)));
    assert_eq!(existing.operation().redeem_request_id, "redeem-1");
    assert_eq!(existing.operation().created_at, 10);

    let conflict = storage
        .claim_reset_credit_operation("operation-1", "account-2", "redeem-2", 12)
        .expect("detect account conflict");
    assert!(matches!(
        conflict,
        ResetCreditOperationClaim::AccountConflict(_)
    ));
    assert_eq!(conflict.operation().account_id, "account-1");
}

#[test]
fn first_terminal_update_wins_and_is_replayable() {
    let storage = storage();
    storage
        .claim_reset_credit_operation("complete-1", "account-1", "redeem-1", 10)
        .unwrap();

    let completed = storage
        .complete_reset_credit_operation("complete-1", "account-1", r#"{"consumed":true}"#, 20)
        .unwrap();
    assert!(matches!(completed, ResetCreditOperationUpdate::Updated(_)));
    assert_eq!(
        completed.operation().unwrap().status,
        ResetCreditOperationStatus::Completed
    );
    assert_eq!(
        completed.operation().unwrap().result_json.as_deref(),
        Some(r#"{"consumed":true}"#)
    );

    let repeated = storage
        .complete_reset_credit_operation("complete-1", "account-1", "ignored", 30)
        .unwrap();
    assert!(matches!(repeated, ResetCreditOperationUpdate::Existing(_)));
    assert_eq!(repeated.operation().unwrap().updated_at, 20);
    assert_eq!(
        repeated.operation().unwrap().result_json.as_deref(),
        Some(r#"{"consumed":true}"#)
    );

    let enriched = storage
        .update_completed_reset_credit_operation_result(
            "complete-1",
            "account-1",
            r#"{"consumed":true,"remaining":2}"#,
            35,
        )
        .unwrap();
    assert!(matches!(enriched, ResetCreditOperationUpdate::Updated(_)));
    assert_eq!(
        enriched.operation().unwrap().result_json.as_deref(),
        Some(r#"{"consumed":true,"remaining":2}"#)
    );

    let late_failure = storage
        .fail_reset_credit_operation("complete-1", "account-1", "late error", 40)
        .unwrap();
    assert!(matches!(
        late_failure,
        ResetCreditOperationUpdate::Existing(_)
    ));
    assert_eq!(
        late_failure.operation().unwrap().status,
        ResetCreditOperationStatus::Completed
    );

    let conflict = storage
        .fail_reset_credit_operation("complete-1", "account-2", "wrong account", 50)
        .unwrap();
    assert!(matches!(
        conflict,
        ResetCreditOperationUpdate::AccountConflict(_)
    ));
    assert!(matches!(
        storage
            .fail_reset_credit_operation("missing", "account-1", "missing", 60)
            .unwrap(),
        ResetCreditOperationUpdate::NotFound
    ));
    assert!(matches!(
        storage
            .update_completed_reset_credit_operation_result("missing", "account-1", "{}", 60)
            .unwrap(),
        ResetCreditOperationUpdate::NotFound
    ));
}

#[test]
fn failed_operation_is_persisted() {
    let storage = storage();
    storage
        .claim_reset_credit_operation("failed-1", "account-1", "redeem-1", 10)
        .unwrap();
    let failed = storage
        .fail_reset_credit_operation("failed-1", "account-1", "provider rejected", 20)
        .unwrap();
    assert!(matches!(failed, ResetCreditOperationUpdate::Updated(_)));
    let operation = storage
        .get_reset_credit_operation("failed-1")
        .unwrap()
        .unwrap();
    assert_eq!(operation.status, ResetCreditOperationStatus::Failed);
    assert_eq!(operation.error.as_deref(), Some("provider rejected"));
    assert_eq!(operation.result_json, None);
    assert!(matches!(
        storage
            .update_completed_reset_credit_operation_result(
                "failed-1",
                "account-1",
                r#"{"consumed":true}"#,
                30
            )
            .unwrap(),
        ResetCreditOperationUpdate::Existing(_)
    ));
}

#[test]
fn pending_operation_blocks_a_new_operation_for_the_same_account() {
    let storage = storage();
    storage
        .claim_reset_credit_operation("pending-1", "account-1", "redeem-1", 10)
        .unwrap();

    let blocked = storage
        .claim_reset_credit_operation("pending-2", "account-1", "redeem-2", 20)
        .unwrap();
    assert!(matches!(
        blocked,
        ResetCreditOperationClaim::PendingAccount(_)
    ));
    assert_eq!(blocked.operation().operation_id, "pending-1");

    storage
        .complete_reset_credit_operation("pending-1", "account-1", r#"{"consumed":true}"#, 30)
        .unwrap();
    let next = storage
        .claim_reset_credit_operation("pending-2", "account-1", "redeem-2", 40)
        .unwrap();
    assert!(matches!(next, ResetCreditOperationClaim::Created(_)));
}

#[test]
fn concurrent_claims_have_one_creator() {
    let storage = storage();
    let barrier = Arc::new(Barrier::new(8));
    let handles = (0..8)
        .map(|index| {
            let storage = storage.shared_handle();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                storage
                    .claim_reset_credit_operation(
                        "concurrent-1",
                        "account-1",
                        &format!("redeem-{index}"),
                        10 + index,
                    )
                    .unwrap()
            })
        })
        .collect::<Vec<_>>();
    let claims = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        claims
            .iter()
            .filter(|claim| matches!(claim, ResetCreditOperationClaim::Created(_)))
            .count(),
        1
    );
    assert!(claims.iter().all(|claim| {
        claim.operation().redeem_request_id == claims[0].operation().redeem_request_id
    }));
}

#[test]
fn concurrent_distinct_claims_have_one_creator_per_account() {
    let storage = storage();
    let barrier = Arc::new(Barrier::new(8));
    let handles = (0..8)
        .map(|index| {
            let storage = storage.shared_handle();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                storage
                    .claim_reset_credit_operation(
                        &format!("concurrent-distinct-{index}"),
                        "account-distinct",
                        &format!("redeem-distinct-{index}"),
                        10 + index,
                    )
                    .unwrap()
            })
        })
        .collect::<Vec<_>>();
    let claims = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        claims
            .iter()
            .filter(|claim| matches!(claim, ResetCreditOperationClaim::Created(_)))
            .count(),
        1
    );
    let created = claims
        .iter()
        .find_map(|claim| match claim {
            ResetCreditOperationClaim::Created(operation) => Some(operation.clone()),
            _ => None,
        })
        .expect("one distinct claim should create the pending operation");
    assert!(claims.iter().all(|claim| {
        claim.operation().account_id == "account-distinct"
            && (claim.operation().operation_id == created.operation_id
                || matches!(claim, ResetCreditOperationClaim::PendingAccount(_)))
    }));

    storage
        .complete_reset_credit_operation(
            &created.operation_id,
            "account-distinct",
            r#"{"consumed":true}"#,
            30,
        )
        .unwrap();
    let replacement = storage
        .claim_reset_credit_operation(
            "concurrent-distinct-replacement",
            "account-distinct",
            "redeem-replacement",
            40,
        )
        .unwrap();
    assert!(matches!(replacement, ResetCreditOperationClaim::Created(_)));
}

#[test]
fn malformed_identifiers_are_rejected() {
    let storage = storage();
    for (operation_id, account_id, redeem_request_id) in [
        ("", "account", "redeem"),
        (" operation", "account", "redeem"),
        ("operation", "", "redeem"),
        ("operation", "account", "redeem "),
    ] {
        assert!(storage
            .claim_reset_credit_operation(operation_id, account_id, redeem_request_id, 1)
            .is_err());
    }
}
