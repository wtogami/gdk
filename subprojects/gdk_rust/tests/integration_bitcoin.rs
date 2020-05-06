use bitcoin::{Address, Amount};
use bitcoincore_rpc::{Auth, Client, RpcApi};
use gdk_common::mnemonic::Mnemonic;
use gdk_common::model::*;
use gdk_common::session::Session;
use gdk_common::Network;
use gdk_electrum::{determine_electrum_url_from_net, ElectrumSession};
use log::LevelFilter;
use log::{Metadata, Record};
use serde_json::Value;
use std::net::TcpStream;
use std::process::Command;
use std::str::FromStr;
use std::time::Duration;
use std::{env, thread};
use tempdir::TempDir;

static LOGGER: SimpleLogger = SimpleLogger;

// ELECTRS_EXEC=/Users/casatta/github/romanz/electrs/target/release/electrs BITCOIND_EXEC=bitcoind WALLY_DIR=nothing cargo test
#[test]
fn integration_bitcoin() {
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
    let mut bitcoind = Command::new(bitcoind_exec)
        .arg(datadir_arg)
        .arg(rpcport_arg)
        .arg("-daemon")
        .arg("-regtest")
        .spawn()
        .unwrap();
    println!("Bitcoin spawned");

    // wait bitcoind is ready, use default wallet
    let client: Client = loop {
        thread::sleep(Duration::from_millis(500));
        assert!(bitcoind.stderr.is_none());
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
    let mut electrs = Command::new(electrs_exec)
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
    println!("Electrs spawned {:?}", electrs.stdout);

    let node_address = client.get_new_address(None, None).unwrap();
    let blocks = client.generate_to_address(101, &node_address).unwrap();
    println!("blocks {:?}", &blocks);

    //TODO do the wait like bitcoind
    thread::sleep(Duration::from_secs(5));
    println!("Electrs after 5 secs {:?}", electrs.stdout);
    let mut electrum_client = electrum_client::Client::new(electrs_url).unwrap();
    let header = electrum_client.block_headers_subscribe().unwrap();
    println!("header {:?}", &header);

    let mut network = Network::default();
    network.url = Some(electrs_url.to_string());
    network.sync_interval = Some(1);
    let db_root = format!("{}", TempDir::new("db_test").unwrap().path().display());
    let url = determine_electrum_url_from_net(&network).unwrap();

    let mut session = ElectrumSession::create_session(network, &db_root, url);

    let mnemonic: Mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about".to_string().into();
    session.login(&mnemonic, None).unwrap();

    let ap = session.get_receive_address(&Value::Null).unwrap();
    assert_eq!(ap.pointer, 1);

    println!("{:?}", ap.address);
    let amount = Amount::from_sat(100000);
    let txid = client
        .send_to_address(
            &Address::from_str(&ap.address).unwrap(),
            amount,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();

    println!("{:?}", txid);

    thread::sleep(Duration::from_secs(7));
    let balances = session.get_balance(0, None).unwrap();

    println!("{:?}", balances);
    assert_eq!(*balances.get("btc").unwrap() as u64, amount.as_sat());

    let mut create_opt = CreateTransaction::default();
    create_opt.addressees.push(AddressAmount {
        address: node_address.to_string(),
        satoshi: 1000,
        asset_tag: None,
    });
    println!("{:?}", create_opt);
    let tx = session.create_transaction(&mut create_opt).unwrap();
    println!("{:?}", tx);
    let signed_tx = session.sign_transaction(&tx).unwrap();
    println!("{:?}", signed_tx);
    let txid = session.broadcast_transaction(&signed_tx.hex).unwrap();
    println!("{:?}", txid);
    client.generate_to_address(1, &node_address).unwrap();

    client.stop().unwrap();
    bitcoind.wait().unwrap();
    println!("Electrs will be killed {:?}", electrs.stdout);
    electrs.kill().unwrap();
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
