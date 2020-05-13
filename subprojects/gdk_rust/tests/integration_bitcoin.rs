use bitcoin::consensus::encode::deserialize;
use bitcoin::{Address, Amount, Transaction, Txid};
use bitcoincore_rpc::{Auth, Client, RpcApi};
use electrum_client::client::ElectrumPlaintextStream;
use gdk_common::mnemonic::Mnemonic;
use gdk_common::model::*;
use gdk_common::session::Session;
use gdk_common::Network;
use gdk_electrum::error::Error;
use gdk_electrum::{determine_electrum_url_from_net, ElectrumSession};
use log::LevelFilter;
use log::{info, Metadata, Record};
use serde_json::Value;
use std::net::TcpStream;
use std::process::Child;
use std::process::Command;
use std::str::FromStr;
use std::time::Duration;
use std::{env, thread};
use tempdir::TempDir;

static LOGGER: SimpleLogger = SimpleLogger;

#[allow(unused)]
struct TestSession {
    node: Client,
    electrs: electrum_client::Client<ElectrumPlaintextStream>,
    electrs_header: electrum_client::Client<ElectrumPlaintextStream>,
    session: ElectrumSession,
    status: u64,
    bitcoind_process: Child,
    electrs_process: Child,
    bitcoin_work_dir: TempDir,
    electrs_work_dir: TempDir,
}

/// launch test wiht env vars eg:
/// ELECTRS_EXEC=$HOME/github/romanz/electrs/target/release/electrs \
/// BITCOIND_EXEC=bitcoind \
/// WALLY_DIR=build-clang/libwally-core/build/lib/ \
/// cargo test

#[test]
fn integration_bitcoin() {
    let mut test_session = setup();

    let node_address = test_session.node_address();
    test_session.fund(100_000_000);
    test_session.send_tx(&node_address, 10_000);
    test_session.send_all(&node_address);
    test_session.mine_block();
    test_session.send_tx_same_script();
    test_session.fund(100_000_000);
    test_session.send_multi(3, 100_000);
    test_session.send_multi(30, 100_000);
    test_session.mine_block();
    test_session.send_fails();
    test_session.fees();
    test_session.settings();

    test_session.stop();
}

//TODO duplicated why I cannot import?
pub struct SimpleLogger;

impl log::Log for SimpleLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            if record.level() <= LevelFilter::Warn {
                println!("{} - {}", record.level(), record.args());
            } else {
                println!("{}", record.args());
            }
        }
    }

    fn flush(&self) {}
}

fn setup() -> TestSession {
    let electrs_exec = env::var("ELECTRS_EXEC")
        .expect("env ELECTRS_EXEC pointing to electrs executable is required");
    let bitcoind_exec = env::var("BITCOIND_EXEC")
        .expect("env BITCOIND_EXEC pointing to bitcoind executable is required");
    env::var("WALLY_DIR").expect("env WALLY_DIR directory containing libwally is required");

    log::set_logger(&LOGGER)
        .map(|()| log::set_max_level(LevelFilter::Info))
        .expect("cannot initialize logging");

    let bitcoin_work_dir = TempDir::new("bitcoin_test").unwrap();

    let cookie_file = bitcoin_work_dir.path().join("regtest").join(".cookie");
    let cookie_file_str = format!("{}", cookie_file.display());

    let rpc_port = 18443u16;
    let socket = format!("127.0.0.1:{}", rpc_port);
    let node_url = format!("http://{}", socket);

    let test = TcpStream::connect(&socket);
    assert!(test.is_err(), "check the port is not open with a previous instance of bitcoind");

    let datadir_arg = format!("-datadir={}", &bitcoin_work_dir.path().display());
    dbg!(&datadir_arg);
    let rpcport_arg = format!("-rpcport={}", rpc_port);
    dbg!(&rpcport_arg);
    let bitcoind_process = Command::new(bitcoind_exec)
        .arg(datadir_arg)
        .arg(rpcport_arg)
        .arg("-daemon")
        .arg("-regtest")
        .spawn()
        .unwrap();
    println!("Bitcoin spawned");

    // wait bitcoind is ready, use default wallet
    let node: Client = loop {
        thread::sleep(Duration::from_millis(500));
        assert!(bitcoind_process.stderr.is_none());
        let client_result = Client::new(node_url.clone(), Auth::CookieFile(cookie_file.clone()));
        match client_result {
            Ok(client) => match client.get_blockchain_info() {
                Ok(_) => break client,
                Err(e) => println!("{:?}", e),
            },
            Err(e) => println!("{:?}", e),
        }
    };
    println!("Bitcoin started");

    let electrs_work_dir = TempDir::new("electrs_test").unwrap();
    let electrs_url = "127.0.0.1:60401";
    let electrs_process = Command::new(electrs_exec)
        .arg("-vvv")
        .arg("--db-dir")
        .arg(format!("{}", electrs_work_dir.path().display()))
        .arg("--daemon-dir")
        .arg(format!("{}", &bitcoin_work_dir.path().display()))
        .arg("--cookie-file")
        .arg(cookie_file_str)
        .arg("--electrum-rpc-addr")
        .arg(electrs_url)
        .arg("--network")
        .arg("regtest")
        .spawn()
        .unwrap();
    println!("Electrs spawned");

    let node_address = node.get_new_address(None, None).unwrap();
    let blocks = node.generate_to_address(101, &node_address).unwrap();
    println!("blocks {:?}", &blocks);

    let electrs = loop {
        match electrum_client::Client::new(electrs_url) {
            Ok(c) => break c,
            Err(_) => thread::sleep(Duration::from_millis(500)),
        }
    };
    let mut electrs_header = electrum_client::Client::new(electrs_url).unwrap();
    let header = electrs_header.block_headers_subscribe().unwrap();
    assert_eq!(header.height, 101);

    let mut network = Network::default();
    network.url = Some(electrs_url.to_string());
    network.sync_interval = Some(1);
    let db_root = format!("{}", TempDir::new("db_test").unwrap().path().display());
    let url = determine_electrum_url_from_net(&network).unwrap();

    let mut session = ElectrumSession::create_session(network, &db_root, url);

    let mnemonic: Mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about".to_string().into();
    session.login(&mnemonic, None).unwrap();

    let status = session.status().unwrap();
    assert_eq!(status, 9288996555440648771);
    TestSession {
        status,
        node,
        electrs,
        electrs_header,
        session,
        bitcoind_process,
        electrs_process,
        bitcoin_work_dir,
        electrs_work_dir,
    }
}

impl TestSession {
    /// wait gdk session status to change (new tx)
    fn wait_status_change(&mut self) {
        loop {
            let new_status = self.session.status().unwrap();
            if self.status != new_status {
                self.status = new_status;
                break;
            }
            thread::sleep(Duration::from_millis(500));
        }
    }

    fn fees(&mut self) {
        let fees = self.session.get_fee_estimates().unwrap();
        let relay_fee = self.node.get_network_info().unwrap().relay_fee.as_sat();
        assert!(fees.iter().all(|f| f.0 >= relay_fee));
    }

    fn settings(&mut self) {
        let mut settings = self.session.get_settings().unwrap();
        settings.altimeout += 1;
        self.session.change_settings(&settings).unwrap();
        let new_settings = self.session.get_settings().unwrap();
        assert_eq!(settings, new_settings);
    }

    /// fund the gdk session with satoshis from the node
    fn fund(&mut self, satoshi: u64) {
        let initial_satoshis = self.satoshi();
        let ap = self.session.get_receive_address(&Value::Null).unwrap();
        let address = Address::from_str(&ap.address).unwrap();
        client_send_to_address(&self.node, &address, satoshi);

        self.wait_status_change();

        assert_eq!(self.satoshi(), initial_satoshis + satoshi);
    }

    /// send all of the balance of the  tx from the gdk session to the specified address
    fn send_all(&mut self, address: &Address) {
        let init_sat = self.satoshi();
        let init_sat_addr = self.satoshi_addr(address);
        let mut create_opt = CreateTransaction::default();
        let fee_rate = 1000;
        create_opt.fee_rate = Some(fee_rate);
        create_opt.addressees.push(AddressAmount {
            address: address.to_string(),
            satoshi: 0,
            asset_tag: None,
        });
        create_opt.send_all = Some(true);
        let tx = self.session.create_transaction(&mut create_opt).unwrap();
        let signed_tx = self.session.sign_transaction(&tx).unwrap();
        self.check_fee_rate(fee_rate, &signed_tx);
        self.session.broadcast_transaction(&signed_tx.hex).unwrap();
        self.wait_status_change();
        let end_sat_addr = self.satoshi_addr(address);
        assert_eq!(init_sat_addr + init_sat - tx.fee, end_sat_addr);
        assert_eq!(self.satoshi(), 0);
    }

    /// send a tx from the gdk session to the specified address
    fn send_tx(&mut self, address: &Address, satoshi: u64) {
        let init_sat = self.satoshi();
        let init_sat_addr = self.satoshi_addr(address);
        let mut create_opt = CreateTransaction::default();
        let fee_rate = 1000;
        create_opt.fee_rate = Some(fee_rate);
        create_opt.addressees.push(AddressAmount {
            address: address.to_string(),
            satoshi,
            asset_tag: None,
        });
        let tx = self.session.create_transaction(&mut create_opt).unwrap();
        let signed_tx = self.session.sign_transaction(&tx).unwrap();
        self.check_fee_rate(fee_rate, &signed_tx);
        self.session.broadcast_transaction(&signed_tx.hex).unwrap();
        self.wait_status_change();
        let end_sat_addr = self.satoshi_addr(address);
        assert_eq!(init_sat_addr + satoshi, end_sat_addr);
        assert_eq!(self.satoshi(), init_sat - satoshi - tx.fee);
    }

    /// send a tx with multiple recipients with same amount from the gdk session to generated
    /// node's addressees
    fn send_multi(&mut self, recipients: u8, amount: u64) {
        let init_sat = self.satoshi();
        let mut create_opt = CreateTransaction::default();
        let fee_rate = 1000;
        create_opt.fee_rate = Some(fee_rate);
        let mut addressees = vec![];
        for _ in 0..recipients {
            let address = self.node_address();
            create_opt.addressees.push(AddressAmount {
                address: address.to_string(),
                satoshi: amount,
                asset_tag: None,
            });
            addressees.push(address);
        }
        let tx = self.session.create_transaction(&mut create_opt).unwrap();
        let signed_tx = self.session.sign_transaction(&tx).unwrap();
        self.check_fee_rate(fee_rate, &signed_tx);
        self.session.broadcast_transaction(&signed_tx.hex).unwrap();
        self.wait_status_change();
        for el in addressees {
            assert_eq!(amount, self.satoshi_addr(&el))
        }
        assert_eq!(init_sat - tx.fee - recipients as u64 * amount, self.satoshi());
    }

    /// send a tx, check it spend utxo with the same script_pubkey together
    fn send_tx_same_script(&mut self) {
        let init_sat = self.satoshi();
        assert_eq!(init_sat, 0);

        let utxo_satoshi = 100_000;
        let ap = self.session.get_receive_address(&Value::Null).unwrap();
        let address = Address::from_str(&ap.address).unwrap();
        client_send_to_address(&self.node, &address, utxo_satoshi);
        client_send_to_address(&self.node, &address, utxo_satoshi);

        self.wait_status_change();
        let satoshi = 50_000; // one utxo would be enough
        let mut create_opt = CreateTransaction::default();
        let fee_rate = 1000;
        let address = self.node_address();
        create_opt.fee_rate = Some(fee_rate);
        create_opt.addressees.push(AddressAmount {
            address: address.to_string(),
            satoshi,
            asset_tag: None,
        });
        let tx = self.session.create_transaction(&mut create_opt).unwrap();
        let signed_tx = self.session.sign_transaction(&tx).unwrap();
        self.check_fee_rate(fee_rate, &signed_tx);
        self.session.broadcast_transaction(&signed_tx.hex).unwrap();
        self.wait_status_change();
        let transaction: Transaction = deserialize(&hex::decode(&signed_tx.hex).unwrap()).unwrap();
        assert_eq!(2, transaction.input.len());
    }

    /// check send failure reasons
    fn send_fails(&mut self) {
        let init_sat = self.satoshi();
        let mut create_opt = CreateTransaction::default();
        let fee_rate = 1000;
        let address = self.node_address();
        create_opt.fee_rate = Some(fee_rate);
        create_opt.addressees.push(AddressAmount {
            address: address.to_string(),
            satoshi: 0,
            asset_tag: None,
        });
        match self.session.create_transaction(&mut create_opt) {
            Err(Error::InvalidAmount) => assert!(true),
            _ => assert!(false),
        }
        create_opt.addressees[0].satoshi = init_sat;
        match self.session.create_transaction(&mut create_opt) {
            Err(Error::InsufficientFunds) => assert!(true),
            _ => assert!(false),
        }
        create_opt.addressees[0].address = "x".to_string();
        match self.session.create_transaction(&mut create_opt) {
            Err(Error::InvalidAddress) => assert!(true),
            _ => assert!(false),
        }
        create_opt.addressees.clear();
        match self.session.create_transaction(&mut create_opt) {
            Err(Error::EmptyAddressees) => assert!(true),
            _ => assert!(false),
        }
    }

    /// mine a block with the node and check if gdk session see the change
    fn mine_block(&mut self) {
        let initial_height = self.electrs_tip();
        info!("mine_block initial_height {}", initial_height);
        let address = self.node_address();
        self.node.generate_to_address(1, &address).unwrap();
        self.wait_status_change();
        let new_height = loop {
            // apparently even if gdk session status changed (thus new height come in)
            // it could happend this is the old height (maybe due to caching) thus we loop wait
            let new_height = self.electrs_tip();
            if new_height != initial_height {
                break new_height;
            }
            info!("height still the same");
            thread::sleep(Duration::from_millis(500));
        };
        info!("mine_block new_height {}", new_height);
        assert_eq!(initial_height + 1, new_height);
    }

    fn node_address(&self) -> Address {
        self.node.get_new_address(None, None).unwrap()
    }

    fn check_fee_rate(&self, req_rate: u64, tx_meta: &TransactionMeta) {
        let transaction: Transaction = deserialize(&hex::decode(&tx_meta.hex).unwrap()).unwrap();
        let real_rate = tx_meta.fee as f64 / (transaction.get_weight() as f64 / 4.0);
        let req_rate = req_rate as f64 / 1000.0;
        let max_perc_diff = 0.90; // TODO improve fee estimation and decrease this
        assert!(
            ((real_rate - req_rate).abs() / real_rate) < max_perc_diff,
            format!("real_rate:{} req_rate:{}", real_rate, req_rate)
        ); // percentage difference between fee rate requested vs real fee
        let relay_fee = self.node.get_network_info().unwrap().relay_fee.as_sat() as f64 / 1000.0;
        assert!(real_rate > relay_fee, "fee rate is under relay_fee");
    }

    /// ask the blockcain tip to electrs
    fn electrs_tip(&mut self) -> usize {
        loop {
            match self.electrs_header.block_headers_subscribe() {
                Ok(header) => return header.height,
                Err(_) => println!("err"), // fixme, for some reason it errors once every two try
            }
        }
    }

    /// balance of an address
    fn satoshi_addr(&mut self, address: &Address) -> u64 {
        let mut satoshi = 0;
        let balance = self.electrs.script_get_balance(&address.script_pubkey()).unwrap();
        if let Some(v) = balance.confirmed {
            satoshi += v;
        }
        if let Some(v) = balance.unconfirmed {
            satoshi += v;
        }
        satoshi
    }

    /// balance in satoshi of the gdk session
    fn satoshi(&self) -> u64 {
        let initial_balances = self.session.get_balance(0, None).unwrap();
        *initial_balances.get("btc").unwrap() as u64
    }

    /// stop the bitcoin node in the test session
    fn stop(&mut self) {
        self.node.stop().unwrap();
        self.bitcoind_process.wait().unwrap();
        self.electrs_process.kill().unwrap();
    }
}

fn client_send_to_address(client: &Client, address: &Address, satoshi: u64) -> Txid {
    client
        .send_to_address(&address, Amount::from_sat(satoshi), None, None, None, None, None, None)
        .unwrap()
}
