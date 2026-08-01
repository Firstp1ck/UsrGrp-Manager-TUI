use std::collections::HashMap;

use usrgrp_manager::search::{
    AccountShadowState, ShadowState, parse_shadow_records, truncate_query_bytes,
};

#[test]
fn readable_shadow_without_an_account_record_is_explicitly_unknown() {
    let statuses = parse_shadow_records("alice:!:1:0:30::::\n", 100);
    let state = ShadowState::Known(statuses);
    assert!(matches!(
        state.account_status("missing"),
        AccountShadowState::Unknown { .. }
    ));
}

#[test]
fn zero_last_change_means_must_change_and_is_retained() {
    let statuses = parse_shadow_records("alice:$6$hash:0:0:30::::\n", 100);
    let alice = statuses.get("alice").unwrap();
    assert!(alice.expired);
    assert_eq!(alice.last_change_days, Some(0));
}

#[test]
fn unavailable_shadow_remains_distinct_from_per_account_unknown() {
    let state = ShadowState::Unavailable {
        reason: "permission denied".into(),
    };
    assert!(matches!(
        state.account_status("alice"),
        AccountShadowState::Unavailable { .. }
    ));
}

#[test]
fn multibyte_query_is_bounded_by_valid_utf8_bytes() {
    let query = "é".repeat(200);
    let bounded = truncate_query_bytes(&query);
    assert!(bounded.len() <= 256);
    assert!(bounded.is_char_boundary(bounded.len()));
    assert_eq!(bounded.len(), 256);
}

#[test]
fn known_shadow_status_is_preserved() {
    let mut statuses = HashMap::new();
    statuses.insert("alice".into(), Default::default());
    let state = ShadowState::Known(statuses);
    assert!(matches!(
        state.account_status("alice"),
        AccountShadowState::Known(_)
    ));
}
