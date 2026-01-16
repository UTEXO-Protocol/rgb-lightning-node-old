use bitcoin::hashes::{sha256::Hash as Sha256, Hash};
use rand::RngCore;
use reqwest::StatusCode;
use serde::Serialize;
use std::net::SocketAddr;
use std::path::Path;
use time::OffsetDateTime;

use crate::{
    disk::{read_claimable_htlcs, CLAIMABLE_HTLCS_FNAME},
    error::APIError,
    utils::{hex_str, validate_and_parse_payment_hash, LDK_DIR},
};

use self::routes::HTLC_MIN_MSAT;
use super::*;

const TEST_DIR_BASE: &str = "tmp/hodl_invoice/";

/// Generate a random preimage and its corresponding payment hash.
fn random_preimage_and_hash() -> (String, String) {
    let mut preimage = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut preimage);
    let preimage_hex = hex_str(&preimage);
    let payment_hash = hex_str(&Sha256::hash(&preimage).to_byte_array());
    (preimage_hex, payment_hash)
}

async fn setup_two_nodes_with_channel(
    test_dir_suffix: &str,
    port_offset: u16,
) -> (SocketAddr, SocketAddr, String, String) {
    let test_dir_base = format!("{TEST_DIR_BASE}{test_dir_suffix}/");
    let test_dir_node1 = format!("{test_dir_base}node1");
    let test_dir_node2 = format!("{test_dir_base}node2");
    let node1_port = NODE1_PEER_PORT + port_offset;
    let node2_port = NODE2_PEER_PORT + port_offset;
    let (node1_addr, _) = start_node(&test_dir_node1, node1_port, false).await;
    let (node2_addr, _) = start_node(&test_dir_node2, node2_port, false).await;

    fund_and_create_utxos(node1_addr, None).await;
    fund_and_create_utxos(node2_addr, None).await;

    let node2_pubkey = node_info(node2_addr).await.pubkey;
    let _channel = open_channel(
        node1_addr,
        &node2_pubkey,
        Some(node2_port),
        Some(500000),
        Some(0),
        None,
        None,
    )
    .await;

    (node1_addr, node2_addr, test_dir_node1, test_dir_node2)
}

async fn setup_two_nodes_with_asset_channel(
    test_dir_suffix: &str,
    port_offset: u16,
    asset_channel_amount: u64,
) -> (SocketAddr, SocketAddr, String, String, String) {
    let test_dir_base = format!("{TEST_DIR_BASE}{test_dir_suffix}/");
    let test_dir_node1 = format!("{test_dir_base}node1");
    let test_dir_node2 = format!("{test_dir_base}node2");
    let node1_port = NODE1_PEER_PORT + port_offset;
    let node2_port = NODE2_PEER_PORT + port_offset;
    let (node1_addr, _) = start_node(&test_dir_node1, node1_port, false).await;
    let (node2_addr, _) = start_node(&test_dir_node2, node2_port, false).await;

    fund_and_create_utxos(node1_addr, None).await;
    fund_and_create_utxos(node2_addr, None).await;

    let asset_id = issue_asset_nia(node1_addr).await.asset_id;
    // Create more UTXOs after issuing asset, as asset issuance consumes UTXOs
    fund_and_create_utxos(node1_addr, None).await;

    let node2_pubkey = node_info(node2_addr).await.pubkey;
    let _channel = open_channel(
        node1_addr,
        &node2_pubkey,
        Some(node2_port),
        Some(500000),
        Some(0),
        Some(asset_channel_amount),
        Some(&asset_id),
    )
    .await;

    (
        node1_addr,
        node2_addr,
        test_dir_node1,
        test_dir_node2,
        asset_id,
    )
}

async fn setup_single_node(test_dir_suffix: &str, port_offset: u16) -> (SocketAddr, String) {
    let test_dir_base = format!("{TEST_DIR_BASE}{test_dir_suffix}/");
    let test_dir_node1 = format!("{test_dir_base}node1");
    let node1_port = NODE1_PEER_PORT + port_offset;
    let (node1_addr, _) = start_node(&test_dir_node1, node1_port, false).await;
    fund_and_create_utxos(node1_addr, None).await;
    (node1_addr, test_dir_node1)
}

async fn invoice_post_expect_error<T: Serialize>(
    node_address: SocketAddr,
    path: &str,
    payload: &T,
    expected_status: StatusCode,
    expected_message: &str,
    expected_name: &str,
) {
    let res = reqwest::Client::new()
        .post(format!("http://{node_address}{path}"))
        .json(payload)
        .send()
        .await
        .unwrap();
    check_response_is_nok(res, expected_status, expected_message, expected_name).await;
}

#[allow(clippy::too_many_arguments)]
async fn invoice_hodl_expect_error(
    node_address: SocketAddr,
    amt_msat: Option<u64>,
    expiry_sec: u32,
    payment_hash: String,
    asset_id: Option<String>,
    asset_amount: Option<u64>,
    expected_status: StatusCode,
    expected_message: &str,
    expected_name: &str,
) {
    println!("creating HODL invoice on node {node_address}");
    let payload = InvoiceHodlRequest {
        amt_msat,
        expiry_sec,
        asset_id,
        asset_amount,
        payment_hash,
        external_ref: None,
    };
    invoice_post_expect_error(
        node_address,
        "/invoice/hodl",
        &payload,
        expected_status,
        expected_message,
        expected_name,
    )
    .await;
}

async fn invoice_settle_expect_error(
    node_address: SocketAddr,
    payment_hash: String,
    payment_preimage: String,
    expected_status: StatusCode,
    expected_message: &str,
    expected_name: &str,
) {
    println!("settling HODL invoice {payment_hash} on node {node_address}");
    let payload = InvoiceSettleRequest {
        payment_hash,
        payment_preimage,
    };
    invoice_post_expect_error(
        node_address,
        "/invoice/settle",
        &payload,
        expected_status,
        expected_message,
        expected_name,
    )
    .await;
}

async fn invoice_cancel_expect_error(
    node_address: SocketAddr,
    payment_hash: String,
    expected_status: StatusCode,
    expected_message: &str,
    expected_name: &str,
) {
    println!("cancelling HODL invoice {payment_hash} on node {node_address}");
    let payload = InvoiceCancelRequest { payment_hash };
    invoice_post_expect_error(
        node_address,
        "/invoice/cancel",
        &payload,
        expected_status,
        expected_message,
        expected_name,
    )
    .await;
}

async fn invoice_cancel_expect_settling_or_settled(node_address: SocketAddr, payment_hash: String) {
    println!("cancelling HODL invoice {payment_hash} on node {node_address}");
    let payload = InvoiceCancelRequest { payment_hash };
    let res = reqwest::Client::new()
        .post(format!("http://{node_address}/invoice/cancel"))
        .json(&payload)
        .send()
        .await
        .unwrap();

    // Racy by nature: if settlement completes first, we see 409 instead of 403.
    if res.status() == StatusCode::FORBIDDEN {
        check_response_is_nok(
            res,
            StatusCode::FORBIDDEN,
            "Invoice settlement is in progress",
            "InvoiceSettlingInProgress",
        )
        .await;
    } else if res.status() == StatusCode::CONFLICT {
        check_response_is_nok(
            res,
            StatusCode::CONFLICT,
            "Invoice is already settled",
            "InvoiceAlreadySettled",
        )
        .await;
    } else {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        panic!("expected 403 settling-in-progress or 409 already settled, got {status}: {body}");
    }
}

fn expect_api_ok<T>(result: Result<T, APIError>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(err) => panic!("{context}: {err}"),
    }
}

/// Check if the claimable HTLC entry exists in the node's on-disk store.
fn claimable_exists(node_test_dir: &str, payment_hash_hex: &str) -> Result<bool, APIError> {
    let claimable_path = Path::new(node_test_dir)
        .join(LDK_DIR)
        .join(CLAIMABLE_HTLCS_FNAME);
    let storage = read_claimable_htlcs(&claimable_path);
    let hash = validate_and_parse_payment_hash(payment_hash_hex)?;
    Ok(storage.payments.contains_key(&hash))
}

/// Get the claim_deadline_height for a claimable, if present on disk.
fn claimable_deadline_height(
    node_test_dir: &str,
    payment_hash_hex: &str,
) -> Result<Option<u32>, APIError> {
    let claimable_path = Path::new(node_test_dir)
        .join(LDK_DIR)
        .join(CLAIMABLE_HTLCS_FNAME);
    let storage = read_claimable_htlcs(&claimable_path);
    let hash = validate_and_parse_payment_hash(payment_hash_hex)?;
    Ok(storage
        .payments
        .get(&hash)
        .and_then(|c| c.claim_deadline_height))
}

/// Check if the claimable entry is marked as settling in storage.
fn claimable_is_settling(node_test_dir: &str, payment_hash_hex: &str) -> Result<bool, APIError> {
    let claimable_path = Path::new(node_test_dir)
        .join(LDK_DIR)
        .join(CLAIMABLE_HTLCS_FNAME);
    let storage = read_claimable_htlcs(&claimable_path);
    let hash = validate_and_parse_payment_hash(payment_hash_hex)?;
    Ok(storage
        .payments
        .get(&hash)
        .and_then(|c| c.settling)
        .unwrap_or(false))
}

/// Poll until the claimable entry appears or disappears (bounded by timeout).
async fn wait_for_claimable_state(
    node_test_dir: &str,
    payment_hash_hex: &str,
    expected: bool,
) -> Result<(), APIError> {
    let t_0 = OffsetDateTime::now_utc();
    loop {
        if claimable_exists(node_test_dir, payment_hash_hex)? == expected {
            return Ok(());
        }
        if (OffsetDateTime::now_utc() - t_0).as_seconds_f32() > 20.0 {
            return Err(APIError::Unexpected(format!(
                "claimable entry for {payment_hash_hex} did not reach state {expected}"
            )));
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

/// Poll until the claimable entry is marked as settling (bounded by timeout).
async fn wait_for_claimable_settling(
    node_test_dir: &str,
    payment_hash_hex: &str,
) -> Result<(), APIError> {
    let t_0 = OffsetDateTime::now_utc();
    loop {
        if claimable_is_settling(node_test_dir, payment_hash_hex)? {
            return Ok(());
        }
        if (OffsetDateTime::now_utc() - t_0).as_seconds_f32() > 10.0 {
            return Err(APIError::Unexpected(format!(
                "claimable entry for {payment_hash_hex} was not marked settling in time"
            )));
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

async fn wait_for_payment_preimage(
    node_address: SocketAddr,
    payment_hash_hex: &str,
) -> Result<GetPaymentPreimageResponse, APIError> {
    let t_0 = OffsetDateTime::now_utc();
    loop {
        let resp = get_payment_preimage(node_address, payment_hash_hex).await;
        if matches!(resp.status, HTLCStatus::Succeeded) && resp.preimage.is_some() {
            return Ok(resp);
        }
        if (OffsetDateTime::now_utc() - t_0).as_seconds_f32() > 20.0 {
            return Err(APIError::Unexpected(format!(
                "preimage for {payment_hash_hex} was not available in time"
            )));
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

#[serial_test::serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[traced_test]
async fn settle_hodl_invoice() {
    initialize();

    // Arrange: start two nodes, fund, and open a channel.
    let (node1_addr, node2_addr, _test_dir_node1, test_dir_node2) =
        setup_two_nodes_with_channel("settle", 0).await;

    // Arrange: create a HODL invoice with a fixed payment hash.
    let (preimage_hex, payment_hash_hex) = random_preimage_and_hash();
    let InvoiceHodlResponse { invoice, .. } = invoice_hodl(
        node2_addr,
        Some(50_000),
        900,
        payment_hash_hex.clone(),
        None,
        None,
    )
    .await;
    let decoded = decode_ln_invoice(node1_addr, &invoice).await;
    assert_eq!(decoded.payment_hash, payment_hash_hex);

    // Act: pay the invoice; HODL keeps it pending and claimable.
    let _ = send_payment_with_status(node1_addr, invoice.clone(), HTLCStatus::Pending).await;
    assert!(matches!(
        invoice_status(node2_addr, &invoice).await,
        InvoiceStatus::Pending
    ));
    expect_api_ok(
        wait_for_claimable_state(&test_dir_node2, &payment_hash_hex, true).await,
        "wait for claimable entry to appear",
    );
    let payee_payment =
        wait_for_ln_payment(node2_addr, &decoded.payment_hash, HTLCStatus::Claimable).await;
    assert_eq!(payee_payment.status, HTLCStatus::Claimable);

    // Act: settle with the chosen preimage.
    invoice_settle(node2_addr, payment_hash_hex.clone(), preimage_hex.clone()).await;

    // Assert: payer/payee succeed and claimable entry is removed.
    let payer_payment =
        wait_for_ln_payment(node1_addr, &decoded.payment_hash, HTLCStatus::Succeeded).await;
    assert_eq!(payer_payment.status, HTLCStatus::Succeeded);
    let payee_payment =
        wait_for_ln_payment(node2_addr, &decoded.payment_hash, HTLCStatus::Succeeded).await;
    assert_eq!(payee_payment.status, HTLCStatus::Succeeded);
    assert!(matches!(
        invoice_status(node2_addr, &invoice).await,
        InvoiceStatus::Succeeded
    ));
    expect_api_ok(
        wait_for_claimable_state(&test_dir_node2, &payment_hash_hex, false).await,
        "wait for claimable entry to be removed",
    );

    let preimage_resp = expect_api_ok(
        wait_for_payment_preimage(node1_addr, &payment_hash_hex).await,
        "wait for payment preimage to be available",
    );
    assert_eq!(preimage_resp.status, HTLCStatus::Succeeded);
    assert_eq!(preimage_resp.preimage, Some(preimage_hex));
}

#[serial_test::serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[traced_test]
async fn settle_hodl_invoice_rgb() {
    initialize();

    let asset_channel_amount = 100;
    let asset_payment_amount = 10;
    let (node1_addr, node2_addr, _test_dir_node1, test_dir_node2, asset_id) =
        setup_two_nodes_with_asset_channel("settle-rgb", 60, asset_channel_amount).await;

    let initial_ln_balance_node1 = asset_balance_offchain_outbound(node1_addr, &asset_id).await;
    let initial_ln_balance_node2 = asset_balance_offchain_outbound(node2_addr, &asset_id).await;

    let (preimage_hex, payment_hash_hex) = random_preimage_and_hash();
    let InvoiceHodlResponse { invoice, .. } = invoice_hodl(
        node2_addr,
        Some(HTLC_MIN_MSAT),
        900,
        payment_hash_hex.clone(),
        Some(&asset_id),
        Some(asset_payment_amount),
    )
    .await;
    let decoded = decode_ln_invoice(node1_addr, &invoice).await;
    assert_eq!(decoded.payment_hash, payment_hash_hex);
    assert_eq!(decoded.amt_msat, Some(3_000_000));
    assert_eq!(decoded.asset_id, Some(asset_id.to_string()));
    assert_eq!(decoded.asset_amount, Some(asset_payment_amount));

    let _ = send_payment_with_status(node1_addr, invoice.clone(), HTLCStatus::Pending).await;
    expect_api_ok(
        wait_for_claimable_state(&test_dir_node2, &payment_hash_hex, true).await,
        "wait for claimable entry to appear",
    );
    let payee_payment =
        wait_for_ln_payment(node2_addr, &decoded.payment_hash, HTLCStatus::Claimable).await;
    assert_eq!(payee_payment.status, HTLCStatus::Claimable);
    assert_eq!(payee_payment.asset_id, Some(asset_id.to_string()));
    assert_eq!(payee_payment.asset_amount, Some(asset_payment_amount));

    invoice_settle(node2_addr, payment_hash_hex.clone(), preimage_hex.clone()).await;

    let payer_payment =
        wait_for_ln_payment(node1_addr, &decoded.payment_hash, HTLCStatus::Succeeded).await;
    assert_eq!(payer_payment.status, HTLCStatus::Succeeded);
    assert_eq!(payer_payment.asset_id, Some(asset_id.to_string()));
    assert_eq!(payer_payment.asset_amount, Some(asset_payment_amount));
    let payee_payment =
        wait_for_ln_payment(node2_addr, &decoded.payment_hash, HTLCStatus::Succeeded).await;
    assert_eq!(payee_payment.status, HTLCStatus::Succeeded);
    assert_eq!(payee_payment.asset_id, Some(asset_id.to_string()));
    assert_eq!(payee_payment.asset_amount, Some(asset_payment_amount));
    expect_api_ok(
        wait_for_claimable_state(&test_dir_node2, &payment_hash_hex, false).await,
        "wait for claimable entry to be removed",
    );

    wait_for_ln_balance(
        node1_addr,
        &asset_id,
        initial_ln_balance_node1 - asset_payment_amount,
    )
    .await;
    wait_for_ln_balance(
        node2_addr,
        &asset_id,
        initial_ln_balance_node2 + asset_payment_amount,
    )
    .await;
}

/// Idempotency: settling twice should both succeed (LDK/LND behavior).
#[serial_test::serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[traced_test]
async fn settle_twice_succeeds() {
    initialize();

    // Arrange: start two nodes, fund, and open a channel.
    let (node1_addr, node2_addr, _test_dir_node1, test_dir_node2) =
        setup_two_nodes_with_channel("settle-twice", 5).await;

    // Arrange: create a HODL invoice with a fixed payment hash.
    let (preimage_hex, payment_hash_hex) = random_preimage_and_hash();
    let InvoiceHodlResponse { invoice, .. } = invoice_hodl(
        node2_addr,
        Some(45_000),
        900,
        payment_hash_hex.clone(),
        None,
        None,
    )
    .await;
    let decoded = decode_ln_invoice(node1_addr, &invoice).await;
    assert_eq!(decoded.payment_hash, payment_hash_hex);

    // Act: pay the invoice; HODL keeps it pending and claimable.
    let _ = send_payment_with_status(node1_addr, invoice.clone(), HTLCStatus::Pending).await;
    expect_api_ok(
        wait_for_claimable_state(&test_dir_node2, &payment_hash_hex, true).await,
        "wait for claimable entry to appear",
    );
    let payee_payment =
        wait_for_ln_payment(node2_addr, &decoded.payment_hash, HTLCStatus::Claimable).await;
    assert_eq!(payee_payment.status, HTLCStatus::Claimable);

    // Act: first settle with the chosen preimage.
    invoice_settle(node2_addr, payment_hash_hex.clone(), preimage_hex.clone()).await;

    // Assert: payer/payee succeed and claimable entry may be cleaned up later.
    let payer_payment =
        wait_for_ln_payment(node1_addr, &decoded.payment_hash, HTLCStatus::Succeeded).await;
    assert_eq!(payer_payment.status, HTLCStatus::Succeeded);
    let payee_payment =
        wait_for_ln_payment(node2_addr, &decoded.payment_hash, HTLCStatus::Succeeded).await;
    assert_eq!(payee_payment.status, HTLCStatus::Succeeded);
    assert!(matches!(
        invoice_status(node2_addr, &invoice).await,
        InvoiceStatus::Succeeded
    ));

    // Act: settle again with the same preimage; should be idempotent success.
    invoice_settle(node2_addr, payment_hash_hex.clone(), preimage_hex.clone()).await;
    let _ = wait_for_ln_payment(node2_addr, &decoded.payment_hash, HTLCStatus::Succeeded).await;
}

/// Idempotent settle with wrong preimage must fail and not change persisted status.
#[serial_test::serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[traced_test]
async fn settle_twice_wrong_preimage_fails() {
    initialize();

    let (node1_addr, node2_addr, _test_dir_node1, test_dir_node2) =
        setup_two_nodes_with_channel("settle-twice-wrong", 6).await;

    let (preimage_hex, payment_hash_hex) = random_preimage_and_hash();
    let InvoiceHodlResponse { invoice, .. } = invoice_hodl(
        node2_addr,
        Some(45_000),
        900,
        payment_hash_hex.clone(),
        None,
        None,
    )
    .await;
    let decoded = decode_ln_invoice(node1_addr, &invoice).await;

    let _ = send_payment_with_status(node1_addr, invoice.clone(), HTLCStatus::Pending).await;
    expect_api_ok(
        wait_for_claimable_state(&test_dir_node2, &payment_hash_hex, true).await,
        "wait for claimable entry to appear",
    );
    let payee_payment =
        wait_for_ln_payment(node2_addr, &decoded.payment_hash, HTLCStatus::Claimable).await;
    assert_eq!(payee_payment.status, HTLCStatus::Claimable);

    // First settle succeeds.
    invoice_settle(node2_addr, payment_hash_hex.clone(), preimage_hex.clone()).await;
    let _ = wait_for_ln_payment(node2_addr, &decoded.payment_hash, HTLCStatus::Succeeded).await;

    // Second settle with wrong preimage must fail and not alter status.
    let (wrong_preimage_hex, _) = random_preimage_and_hash();
    invoice_settle_expect_error(
        node2_addr,
        payment_hash_hex.clone(),
        wrong_preimage_hex,
        StatusCode::BAD_REQUEST,
        "Invalid payment preimage",
        "InvalidPaymentPreimage",
    )
    .await;
    assert!(matches!(
        invoice_status(node2_addr, &invoice).await,
        InvoiceStatus::Succeeded
    ));
}

/// Idempotent settle after invoice expiry should still succeed.
#[serial_test::serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[traced_test]
async fn settle_after_expiry_idempotent_succeeds() {
    initialize();

    let (node1_addr, node2_addr, _test_dir_node1, test_dir_node2) =
        setup_two_nodes_with_channel("settle-after-expiry", 7).await;

    let (preimage_hex, payment_hash_hex) = random_preimage_and_hash();
    let InvoiceHodlResponse { invoice, .. } = invoice_hodl(
        node2_addr,
        Some(45_000),
        10,
        payment_hash_hex.clone(),
        None,
        None,
    )
    .await;
    let decoded = decode_ln_invoice(node1_addr, &invoice).await;

    let _ = send_payment_with_status(node1_addr, invoice.clone(), HTLCStatus::Pending).await;
    expect_api_ok(
        wait_for_claimable_state(&test_dir_node2, &payment_hash_hex, true).await,
        "wait for claimable entry to appear",
    );
    let payee_payment =
        wait_for_ln_payment(node2_addr, &decoded.payment_hash, HTLCStatus::Claimable).await;
    assert_eq!(payee_payment.status, HTLCStatus::Claimable);

    // Settle before expiry.
    invoice_settle(node2_addr, payment_hash_hex.clone(), preimage_hex.clone()).await;
    let _ = wait_for_ln_payment(node2_addr, &decoded.payment_hash, HTLCStatus::Succeeded).await;

    // Let invoice expiry elapse and call settle again: should still succeed idempotently.
    tokio::time::sleep(std::time::Duration::from_secs(45)).await;
    invoice_settle(node2_addr, payment_hash_hex.clone(), preimage_hex.clone()).await;
    let _ = wait_for_ln_payment(node2_addr, &decoded.payment_hash, HTLCStatus::Succeeded).await;
    assert!(matches!(
        invoice_status(node2_addr, &invoice).await,
        InvoiceStatus::Succeeded
    ));
}

/// Cancel and then try to cancel again (the second call fails).
#[serial_test::serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[traced_test]
async fn cancel_hodl_invoice() {
    initialize();

    // Arrange: start two nodes, fund, and open a channel.
    let (node1_addr, node2_addr, _test_dir_node1, test_dir_node2) =
        setup_two_nodes_with_channel("cancel", 10).await;

    // Arrange: create a HODL invoice with a fixed payment hash.
    let (_preimage_hex, payment_hash_hex) = random_preimage_and_hash();
    let InvoiceHodlResponse { invoice, .. } = invoice_hodl(
        node2_addr,
        Some(40_000),
        900,
        payment_hash_hex.clone(),
        None,
        None,
    )
    .await;
    let decoded = decode_ln_invoice(node1_addr, &invoice).await;
    assert_eq!(decoded.payment_hash, payment_hash_hex);

    // Act: pay the invoice; it should be pending and claimable.
    let _ = send_payment_with_status(node1_addr, invoice.clone(), HTLCStatus::Pending).await;
    assert!(matches!(
        invoice_status(node2_addr, &invoice).await,
        InvoiceStatus::Pending
    ));
    expect_api_ok(
        wait_for_claimable_state(&test_dir_node2, &payment_hash_hex, true).await,
        "wait for claimable entry to appear",
    );
    let payee_payment =
        wait_for_ln_payment(node2_addr, &decoded.payment_hash, HTLCStatus::Claimable).await;
    assert_eq!(payee_payment.status, HTLCStatus::Claimable);

    // Act: cancel and fail back the HTLC.
    invoice_cancel(node2_addr, payment_hash_hex.clone()).await;

    // Assert: payer fails, payee cancels, and claimable entry is removed.
    let payer_payment =
        wait_for_ln_payment(node1_addr, &decoded.payment_hash, HTLCStatus::Failed).await;
    assert_eq!(payer_payment.status, HTLCStatus::Failed);
    let payee_payment =
        wait_for_ln_payment(node2_addr, &decoded.payment_hash, HTLCStatus::Cancelled).await;
    assert_eq!(payee_payment.status, HTLCStatus::Cancelled);
    assert!(matches!(
        invoice_status(node2_addr, &invoice).await,
        InvoiceStatus::Cancelled
    ));
    expect_api_ok(
        wait_for_claimable_state(&test_dir_node2, &payment_hash_hex, false).await,
        "wait for claimable entry to be removed",
    );

    // Duplicate cancel should fail.
    invoice_cancel_expect_error(
        node2_addr,
        payment_hash_hex,
        StatusCode::NOT_FOUND,
        "No claimable HTLC found for this invoice",
        "InvoiceNotClaimable",
    )
    .await;
}

/// Cancelling first must make a later settle fail (already cancelled).
#[serial_test::serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[traced_test]
async fn cancel_then_settle_fails() {
    initialize();

    let (node1_addr, node2_addr, _test_dir_node1, test_dir_node2) =
        setup_two_nodes_with_channel("cancel-settle", 11).await;

    let (preimage_hex, payment_hash_hex) = random_preimage_and_hash();
    let InvoiceHodlResponse { invoice, .. } = invoice_hodl(
        node2_addr,
        Some(40_000),
        900,
        payment_hash_hex.clone(),
        None,
        None,
    )
    .await;
    let decoded = decode_ln_invoice(node1_addr, &invoice).await;

    let _ = send_payment_with_status(node1_addr, invoice.clone(), HTLCStatus::Pending).await;
    expect_api_ok(
        wait_for_claimable_state(&test_dir_node2, &payment_hash_hex, true).await,
        "claimable entry should appear",
    );
    let payee_payment =
        wait_for_ln_payment(node2_addr, &decoded.payment_hash, HTLCStatus::Claimable).await;
    assert_eq!(payee_payment.status, HTLCStatus::Claimable);

    invoice_cancel(node2_addr, payment_hash_hex.clone()).await;

    invoice_settle_expect_error(
        node2_addr,
        payment_hash_hex.clone(),
        preimage_hex,
        StatusCode::NOT_FOUND,
        "No claimable HTLC found for this invoice",
        "InvoiceNotClaimable",
    )
    .await;

    let payee_payment =
        wait_for_ln_payment(node2_addr, &decoded.payment_hash, HTLCStatus::Cancelled).await;
    assert_eq!(payee_payment.status, HTLCStatus::Cancelled);
}

/// Settling first must make a later cancel fail (already settled).
#[serial_test::serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[traced_test]
async fn settle_then_cancel_fails() {
    initialize();

    let (node1_addr, node2_addr, _test_dir_node1, test_dir_node2) =
        setup_two_nodes_with_channel("settle-cancel", 12).await;

    let (preimage_hex, payment_hash_hex) = random_preimage_and_hash();
    let InvoiceHodlResponse { invoice, .. } = invoice_hodl(
        node2_addr,
        Some(42_000),
        900,
        payment_hash_hex.clone(),
        None,
        None,
    )
    .await;
    let decoded = decode_ln_invoice(node1_addr, &invoice).await;

    let _ = send_payment_with_status(node1_addr, invoice.clone(), HTLCStatus::Pending).await;
    expect_api_ok(
        wait_for_claimable_state(&test_dir_node2, &payment_hash_hex, true).await,
        "claimable entry should appear",
    );
    let payee_payment =
        wait_for_ln_payment(node2_addr, &decoded.payment_hash, HTLCStatus::Claimable).await;
    assert_eq!(payee_payment.status, HTLCStatus::Claimable);

    invoice_settle(node2_addr, payment_hash_hex.clone(), preimage_hex).await;
    let payee_payment =
        wait_for_ln_payment(node2_addr, &decoded.payment_hash, HTLCStatus::Succeeded).await;
    assert_eq!(payee_payment.status, HTLCStatus::Succeeded);

    invoice_cancel_expect_error(
        node2_addr,
        payment_hash_hex.clone(),
        StatusCode::CONFLICT,
        "Invoice is already settled",
        "InvoiceAlreadySettled",
    )
    .await;
}

/// Cancel should be rejected while a settle is in progress.
#[serial_test::serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[traced_test]
async fn cancel_while_settling_fails() {
    initialize();

    let (node1_addr, node2_addr, _test_dir_node1, test_dir_node2) =
        setup_two_nodes_with_channel("cancel-while-settling", 13).await;

    let (preimage_hex, payment_hash_hex) = random_preimage_and_hash();
    let InvoiceHodlResponse { invoice, .. } = invoice_hodl(
        node2_addr,
        Some(42_000),
        900,
        payment_hash_hex.clone(),
        None,
        None,
    )
    .await;
    let decoded = decode_ln_invoice(node1_addr, &invoice).await;

    let _ = send_payment_with_status(node1_addr, invoice.clone(), HTLCStatus::Pending).await;
    expect_api_ok(
        wait_for_claimable_state(&test_dir_node2, &payment_hash_hex, true).await,
        "claimable entry should appear",
    );
    let payee_payment =
        wait_for_ln_payment(node2_addr, &decoded.payment_hash, HTLCStatus::Claimable).await;
    assert_eq!(payee_payment.status, HTLCStatus::Claimable);

    invoice_settle(node2_addr, payment_hash_hex.clone(), preimage_hex).await;
    expect_api_ok(
        wait_for_claimable_settling(&test_dir_node2, &payment_hash_hex).await,
        "claimable entry should be marked settling",
    );

    // Prefer the settling-in-progress error; accept already-settled if the race completes first.
    invoice_cancel_expect_settling_or_settled(node2_addr, payment_hash_hex.clone()).await;

    let payee_payment =
        wait_for_ln_payment(node2_addr, &decoded.payment_hash, HTLCStatus::Succeeded).await;
    assert_eq!(payee_payment.status, HTLCStatus::Succeeded);
}

/// Expiry via short invoice timeout: ensure settle/cancel fail after expiry.
#[serial_test::serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[traced_test]
async fn expire_hodl_invoice() {
    initialize();

    // Arrange: start two nodes, fund, and open a channel.
    let (node1_addr, node2_addr, _test_dir_node1, test_dir_node2) =
        setup_two_nodes_with_channel("expiry", 20).await;

    // Arrange: create a short-expiry HODL invoice (20s).
    let (_preimage_hex, payment_hash_hex) = random_preimage_and_hash();
    // Use a small-but-not-too-small expiry to let the payment reach Pending
    // before the background expiry task fails it.
    let InvoiceHodlResponse { invoice, .. } = invoice_hodl(
        node2_addr,
        Some(30_000),
        20,
        payment_hash_hex.clone(),
        None,
        None,
    )
    .await;
    let decoded = decode_ln_invoice(node1_addr, &invoice).await;
    assert_eq!(decoded.payment_hash, payment_hash_hex);

    // Act: pay and wait for the background expiry task to fail the HTLC.
    // Timing note: expiry is 20s, the expiry task runs every 30s, and the payment wait timeout
    // is 40s, so this should succeed on the next expiry tick.
    let _ = send_payment_with_status(node1_addr, invoice.clone(), HTLCStatus::Pending).await;
    expect_api_ok(
        wait_for_claimable_state(&test_dir_node2, &payment_hash_hex, true).await,
        "wait for claimable entry to appear",
    );
    let payee_payment =
        wait_for_ln_payment(node2_addr, &decoded.payment_hash, HTLCStatus::Claimable).await;
    assert_eq!(payee_payment.status, HTLCStatus::Claimable);

    // Assert: both sides see Failed and claimable entry is removed.
    let payer_payment =
        wait_for_ln_payment(node1_addr, &decoded.payment_hash, HTLCStatus::Failed).await;
    assert_eq!(payer_payment.status, HTLCStatus::Failed);
    let payee_payment =
        wait_for_ln_payment(node2_addr, &decoded.payment_hash, HTLCStatus::Failed).await;
    assert_eq!(payee_payment.status, HTLCStatus::Failed);
    assert!(matches!(
        invoice_status(node2_addr, &invoice).await,
        InvoiceStatus::Failed
    ));
    expect_api_ok(
        wait_for_claimable_state(&test_dir_node2, &payment_hash_hex, false).await,
        "wait for claimable entry to be removed",
    );

    // After expiry, settle/cancel should fail.
    invoice_settle_expect_error(
        node2_addr,
        payment_hash_hex.clone(),
        _preimage_hex,
        StatusCode::NOT_FOUND,
        "No claimable HTLC found for this invoice",
        "InvoiceNotClaimable",
    )
    .await;
    invoice_cancel_expect_error(
        node2_addr,
        payment_hash_hex,
        StatusCode::NOT_FOUND,
        "No claimable HTLC found for this invoice",
        "InvoiceNotClaimable",
    )
    .await;
}

/// Expiry driven by CLTV/blocks: mine past deadline, then settle/cancel must fail.
#[serial_test::serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[traced_test]
async fn expire_hodl_invoice_by_blocks() {
    initialize();

    // Arrange: start two nodes, fund, and open a channel.
    let (node1_addr, node2_addr, _test_dir_node1, test_dir_node2) =
        setup_two_nodes_with_channel("expiry-blocks", 25).await;

    // Arrange: create a HODL invoice with standard expiry.
    let (_preimage_hex, payment_hash_hex) = random_preimage_and_hash();
    let InvoiceHodlResponse { invoice, .. } = invoice_hodl(
        node2_addr,
        Some(30_000),
        900,
        payment_hash_hex.clone(),
        None,
        None,
    )
    .await;
    let decoded = decode_ln_invoice(node1_addr, &invoice).await;
    assert_eq!(decoded.payment_hash, payment_hash_hex);

    // Pay and wait for claimable.
    let _ = send_payment_with_status(node1_addr, invoice.clone(), HTLCStatus::Pending).await;
    expect_api_ok(
        wait_for_claimable_state(&test_dir_node2, &payment_hash_hex, true).await,
        "wait for claimable entry to appear",
    );
    let payee_payment =
        wait_for_ln_payment(node2_addr, &decoded.payment_hash, HTLCStatus::Claimable).await;
    assert_eq!(payee_payment.status, HTLCStatus::Claimable);

    // Mine past the claim deadline height (reported by LDK) to force timeout, then
    // give the 30s expiry task a chance to sweep it.
    let deadline_height = claimable_deadline_height(&test_dir_node2, &payment_hash_hex)
        .expect("read deadline height")
        .expect("expected claim_deadline_height for claimable HTLC");
    let current_height = super::get_block_count();
    let blocks_to_mine = deadline_height.saturating_sub(current_height) + 2;
    super::mine_n_blocks(false, blocks_to_mine as u16);
    tokio::time::sleep(std::time::Duration::from_secs(35)).await;

    // Assert: both sides see Failed and claimable entry is removed.
    let payer_payment =
        wait_for_ln_payment(node1_addr, &decoded.payment_hash, HTLCStatus::Failed).await;
    assert_eq!(payer_payment.status, HTLCStatus::Failed);
    let payee_payment =
        wait_for_ln_payment(node2_addr, &decoded.payment_hash, HTLCStatus::Failed).await;
    assert_eq!(payee_payment.status, HTLCStatus::Failed);
    assert!(matches!(
        invoice_status(node2_addr, &invoice).await,
        InvoiceStatus::Failed
    ));
    expect_api_ok(
        wait_for_claimable_state(&test_dir_node2, &payment_hash_hex, false).await,
        "wait for claimable entry to be removed",
    );

    // After expiry, settle/cancel should fail.
    invoice_settle_expect_error(
        node2_addr,
        payment_hash_hex.clone(),
        _preimage_hex,
        StatusCode::NOT_FOUND,
        "No claimable HTLC found for this invoice",
        "InvoiceNotClaimable",
    )
    .await;
    invoice_cancel_expect_error(
        node2_addr,
        payment_hash_hex,
        StatusCode::NOT_FOUND,
        "No claimable HTLC found for this invoice",
        "InvoiceNotClaimable",
    )
    .await;
}

#[serial_test::serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[traced_test]
async fn reject_wrong_preimage_settle() {
    initialize();

    // Arrange: start two nodes, fund, and open a channel.
    let (node1_addr, node2_addr, _test_dir_node1, test_dir_node2) =
        setup_two_nodes_with_channel("wrong_preimage", 30).await;

    // Arrange: create a HODL invoice and pay it (pending).
    let (good_preimage_hex, payment_hash_hex) = random_preimage_and_hash();
    let InvoiceHodlResponse { invoice, .. } = invoice_hodl(
        node2_addr,
        Some(35_000),
        900,
        payment_hash_hex.clone(),
        None,
        None,
    )
    .await;
    let decoded = decode_ln_invoice(node1_addr, &invoice).await;
    assert_eq!(decoded.payment_hash, payment_hash_hex);

    let _ = send_payment_with_status(node1_addr, invoice.clone(), HTLCStatus::Pending).await;
    expect_api_ok(
        wait_for_claimable_state(&test_dir_node2, &payment_hash_hex, true).await,
        "wait for claimable entry to appear",
    );
    let payee_payment =
        wait_for_ln_payment(node2_addr, &decoded.payment_hash, HTLCStatus::Claimable).await;
    assert_eq!(payee_payment.status, HTLCStatus::Claimable);

    // Act: try to settle with a mismatching preimage.
    let (wrong_preimage_hex, _) = random_preimage_and_hash();
    invoice_settle_expect_error(
        node2_addr,
        payment_hash_hex.clone(),
        wrong_preimage_hex,
        StatusCode::BAD_REQUEST,
        "Invalid payment preimage",
        "InvalidPaymentPreimage",
    )
    .await;

    // Assert: invoice stays pending and claimable entry remains.
    assert!(matches!(
        invoice_status(node2_addr, &invoice).await,
        InvoiceStatus::Pending
    ));
    expect_api_ok(
        wait_for_claimable_state(&test_dir_node2, &payment_hash_hex, true).await,
        "wait for claimable entry to remain",
    );
    let payee_payment =
        wait_for_ln_payment(node2_addr, &decoded.payment_hash, HTLCStatus::Claimable).await;
    assert_eq!(payee_payment.status, HTLCStatus::Claimable);

    // Now settle with the correct preimage; should succeed and clean up.
    invoice_settle(node2_addr, payment_hash_hex.clone(), good_preimage_hex).await;
    let _ = wait_for_ln_payment(node2_addr, &decoded.payment_hash, HTLCStatus::Succeeded).await;
    assert!(matches!(
        invoice_status(node2_addr, &invoice).await,
        InvoiceStatus::Succeeded
    ));
}

#[serial_test::serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[traced_test]
async fn reject_duplicate_hodl_payment_hash() {
    initialize();

    // Arrange: start a node and fund it.
    let (node1_addr, _test_dir_node1) = setup_single_node("duplicate_hash", 40).await;

    // Arrange: create the first HODL invoice.
    let (_preimage_hex, payment_hash_hex) = random_preimage_and_hash();
    let InvoiceHodlResponse { invoice, .. } = invoice_hodl(
        node1_addr,
        Some(20_000),
        900,
        payment_hash_hex.clone(),
        None,
        None,
    )
    .await;

    // Act: attempt to create another HODL invoice with the same hash.
    invoice_hodl_expect_error(
        node1_addr,
        Some(20_000),
        900,
        payment_hash_hex.clone(),
        None,
        None,
        StatusCode::BAD_REQUEST,
        "Payment hash already used",
        "PaymentHashAlreadyUsed",
    )
    .await;

    // Assert: the original invoice remains pending.
    assert!(matches!(
        invoice_status(node1_addr, &invoice).await,
        InvoiceStatus::Pending
    ));
}

#[serial_test::serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[traced_test]
async fn auto_claim_invoice_regression() {
    initialize();

    // Arrange: start two nodes, fund, and open a channel.
    let (node1_addr, node2_addr, _test_dir_node1, _test_dir_node2) =
        setup_two_nodes_with_channel("autoclaim", 50).await;

    // Act: create and pay a normal (auto-claim) invoice.
    let LNInvoiceResponse { invoice } = ln_invoice(node2_addr, Some(25_000), None, None, 900).await;
    let decoded = decode_ln_invoice(node1_addr, &invoice).await;

    let _ = send_payment_with_status(node1_addr, invoice.clone(), HTLCStatus::Succeeded).await;
    // Assert: both sides succeed and invoice status updates.
    let payer_payment =
        wait_for_ln_payment(node1_addr, &decoded.payment_hash, HTLCStatus::Succeeded).await;
    assert_eq!(payer_payment.status, HTLCStatus::Succeeded);
    let payee_payment =
        wait_for_ln_payment(node2_addr, &decoded.payment_hash, HTLCStatus::Succeeded).await;
    assert_eq!(payee_payment.status, HTLCStatus::Succeeded);
    assert!(matches!(
        invoice_status(node2_addr, &invoice).await,
        InvoiceStatus::Succeeded
    ));
}
