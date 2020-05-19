use std::env;
mod test_session;

#[test]
fn bitcoin() {
    let electrs_exec = env::var("ELECTRS_EXEC")
        .expect("env ELECTRS_EXEC pointing to electrs executable is required");
    let node_exec = env::var("BITCOIND_EXEC")
        .expect("env BITCOIND_EXEC pointing to elementsd executable is required");
    env::var("WALLY_DIR").expect("env WALLY_DIR directory containing libwally is required");
    let debug = env::var("DEBUG").is_ok();

    let mut test_session = test_session::setup(false, debug, electrs_exec, node_exec);

    let node_address = test_session.node_getnewaddress(None);
    let node_bech32_address = test_session.node_getnewaddress(Some("bech32"));
    test_session.fund(100_000_000, None);
    test_session.send_tx(&node_address, 10_000, None);
    test_session.send_tx(&node_bech32_address, 10_000, None);
    test_session.send_all(&node_address, None);
    test_session.mine_block();
    test_session.send_tx_same_script(None);
    test_session.fund(100_000_000, None);
    test_session.send_multi(3, 100_000, vec![]);
    test_session.send_multi(30, 100_000, vec![]);
    test_session.mine_block();
    test_session.send_fails();
    test_session.fees();
    test_session.settings();

    test_session.stop();
}

#[test]
fn liquid() {
    let electrs_exec = env::var("ELECTRS_LIQUID_EXEC")
        .expect("env ELECTRS_LIQUID_EXEC pointing to electrs executable is required");
    let node_exec = env::var("ELEMENTSD_EXEC")
        .expect("env ELEMENTSD_EXEC pointing to elementsd executable is required");
    env::var("WALLY_DIR").expect("env WALLY_DIR directory containing libwally is required");
    let debug = env::var("DEBUG").is_ok();

    let mut test_session = test_session::setup(true, debug, electrs_exec, node_exec);

    let node_address = test_session.node_getnewaddress(None);
    let node_bech32_address = test_session.node_getnewaddress(Some("bech32"));

    let assets = test_session.fund(100_000_000, Some(1));
    let issued_asset = Some(assets[0].clone());
    test_session.send_tx(&node_address, 10_000, None);
    test_session.send_tx(&node_bech32_address, 10_000, None);
    test_session.send_tx(&node_address, 10_000, issued_asset.clone());
    test_session.send_all(&node_address, issued_asset.clone());
    test_session.send_all(&node_address, test_session.asset_tag());
    test_session.send_tx_same_script(issued_asset.clone());
    test_session.mine_block();

    let assets = test_session.fund(100_000_000, Some(3));
    test_session.send_multi(3, 100_000, vec![]);
    test_session.send_multi(30, 100_000 , assets);
    test_session.mine_block();
    test_session.send_fails();
    test_session.fees();
    test_session.settings();

    test_session.stop();
}