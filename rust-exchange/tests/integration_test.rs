//! Integration tests: end-to-end order lifecycle verification.
//!
//! Full integration tests live inside the api crate (`crates/api/src/main.rs`)
//! because `bootstrap_runtime` and `AppBootstrap` are `pub(crate)`.
//!
//! Run them with: `cargo test --package api -- integration`
//!
//! Tests included:
//!   - integration_full_order_lifecycle: place buy + sell, verify fills & positions
//!   - integration_partial_fill_leaves_resting_order: partial fill, drain remaining
//!   - integration_self_trade_prevention: same user buy+sell blocked
//!   - integration_kill_switch_blocks_orders: kill switch rejects new orders

#[test]
fn see_api_crate_for_integration_tests() {
    // Workspace-level stub. Real tests: `cargo test --package api -- integration`
    assert!(true);
}
