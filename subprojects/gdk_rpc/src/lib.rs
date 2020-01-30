#![recursion_limit = "128"]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

extern crate backtrace;
extern crate bitcoin;
extern crate bitcoin_hashes;
extern crate bitcoincore_rpc;
extern crate chrono;
extern crate dirs;
extern crate gdk_electrum;
extern crate hyper;
extern crate hyper_socks;
extern crate jsonrpc;
extern crate libc;
extern crate rand;
extern crate serde;
#[macro_use]
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate lazy_static;
extern crate failure;
#[macro_use]
extern crate log;
#[cfg(feature = "android_log")]
extern crate android_logger;
#[cfg(feature = "stderr_logger")]
extern crate stderrlog;
extern crate url;

// Liquid
#[cfg(feature = "liquid")]
extern crate elements;
#[cfg(feature = "liquid")]
extern crate liquid_rpc;

pub mod coins;
pub mod constants;
#[macro_use]
pub mod errors;
pub mod network;
pub mod session;
pub mod settings;
pub mod util;
pub mod wallet;
pub mod wally;

use serde_json::{from_value, Value};

#[cfg(feature = "android_log")]
use android_logger::{Config, FilterBuilder};
use bitcoincore_rpc::RpcApi;
use log::Level;
use std::collections::HashMap;
use std::ffi::CString;
use std::mem::transmute;
use std::os::raw::c_char;

#[cfg(feature = "android_log")]
use std::sync::{Once, ONCE_INIT};

use crate::constants::{GA_ERROR, GA_MEMO_USER, GA_NOT_AUTHORIZED, GA_OK, GA_RECONNECT};
use crate::errors::OptionExt;
use crate::network::{Network, RpcConfig};
use crate::session::GDKRPC_session;
use crate::util::{extend, log_filter, make_str, read_str};
use crate::wallet::Wallet;

#[derive(Debug)]
#[repr(C)]
pub struct GDKRPC_json(Value);

impl GDKRPC_json {
    fn new(data: Value) -> *const GDKRPC_json {
        unsafe { transmute(Box::new(GDKRPC_json(data))) }
    }
}

#[derive(Debug)]
#[repr(C)]
pub enum GA_auth_handler {
    Error(String),
    Done(Value),
}

impl GA_auth_handler {
    fn done(res: Value) -> *const GA_auth_handler {
        debug!("GA_auth_handler::done() {:?}", res);
        let handler = GA_auth_handler::Done(res);
        unsafe { transmute(Box::new(handler)) }
    }
    fn success() -> *const GA_auth_handler {
        GA_auth_handler::done(Value::Null)
    }

    fn to_json(&self) -> Value {
        match self {
            GA_auth_handler::Error(err) => json!({ "status": "error", "error": err }),
            GA_auth_handler::Done(res) => json!({ "status": "done", "result": res }),
        }
    }
}

//
// Macros
//

macro_rules! tryit {
    ($x:expr) => {
        match $x {
            Err(err) => {
                println!("error: {:?}", err);
                return GA_ERROR;
            }
            Ok(x) => {
                // can't easily print x because bitcoincore_rpc::Client is not serializable :(
                // should be fixed with https://github.com/rust-bitcoin/rust-bitcoincore-rpc/pull/51
                x
            }
        }
    };
}

macro_rules! ok {
    ($t:expr, $x:expr) => {
        unsafe {
            let x = $x;
            debug!("ok!() {:?}", x);
            *$t = x;
            GA_OK
        }
    };
}

macro_rules! ok_json {
    ($t:expr, $x:expr) => {{
        let x = json!($x);
        debug!("ok_json!() {:?}", x);
        ok!($t, GDKRPC_json::new(x))
    }};
}

macro_rules! safe_ref {
    ($t:expr) => {{
        if $t.is_null() {
            return GA_ERROR;
        }
        unsafe { &*$t }
    }};
}

macro_rules! safe_mut_ref {
    ($t:expr) => {{
        if $t.is_null() {
            return GA_ERROR;
        }
        unsafe { &mut *$t }
    }};
}

//
// Networks
//

#[no_mangle]
pub extern "C" fn GDKRPC_get_networks(
    sess_: *const GDKRPC_session,
    ret: *mut *const GDKRPC_json,
) -> i32 {
    let sess = &safe_ref!(sess_);
    let networks = &sess.networks;
    let names: Vec<String> = networks.keys().cloned().collect();

    let mut networks = json!(networks);
    let networks = networks.as_object_mut().unwrap();
    networks.insert("all_networks".to_string(), json!(names));

    ok_json!(ret, networks)
}
//
// Session & account management
//

#[cfg(feature = "android_log")]
static INIT_LOGGER: Once = ONCE_INIT;

#[no_mangle]
pub extern "C" fn GDKRPC_create_session(
    ret: *mut *const GDKRPC_session,
    networks: *const GDKRPC_json,
) -> i32 {
    debug!("GA_create_session()");

    #[cfg(feature = "android_log")]
    INIT_LOGGER.call_once(|| {
        android_logger::init_once(
            Config::default()
                .with_min_level(Level::Trace)
                .with_filter(FilterBuilder::new().parse("debug,hello::crate=gdk_rpc").build()),
        )
    });

    let networks = &safe_ref!(networks).0;
    let mut rpc_networks: HashMap<String, Network> = HashMap::new();

    if !networks.is_object() {
        println!("Expected networks to be an object");
        return GA_ERROR;
    }

    for (k, network) in networks.as_object().unwrap().iter() {
        if network.get("server_type").map(|v| v.as_str() == Some("rpc")).unwrap_or(false) {
            let parsed_network = serde_json::from_value(network.clone());
            if let Err(msg) = parsed_network {
                println!("Error parsing network '{}': {}", k, msg);
                return GA_ERROR;
            }
            rpc_networks.insert(k.into(), parsed_network.unwrap());
        }
    }

    let sess = GDKRPC_session::new(rpc_networks);

    ok!(ret, sess)
}

#[no_mangle]
pub extern "C" fn GDKRPC_destroy_session(sess: *mut GDKRPC_session) -> i32 {
    let sess = safe_mut_ref!(sess);

    if let Some(wallet) = sess.wallet.take() {
        tryit!(wallet.logout());
    }

    GA_OK
}

fn obj_string(val: &Value, key: &str) -> Option<String> {
    obj_str(val, key).map(|s| s.to_string())
}

fn obj_str<'a>(val: &'a Value, key: &str) -> Option<&'a str> {
    val.get(key).and_then(|v| v.as_str())
}

fn json_to_rpc_config(val: &Value) -> Option<RpcConfig> {
    let url = obj_string(val, "rpc_url")?;
    let user = obj_string(val, "username")?;
    let pass = obj_string(val, "password")?;
    let network = obj_string(val, "name")?;
    let msocks5 = obj_string(val, "socks5");
    Some(RpcConfig {
        url,
        network,
        cred: Some((user, pass)),
        socks5: msocks5,
        cookie: None,
    })
}

#[no_mangle]
pub extern "C" fn GDKRPC_connect(
    sess: *mut GDKRPC_session,
    net_params: *const GDKRPC_json,
    log_level: u32,
) -> i32 {
    let sess = safe_mut_ref!(sess);

    log::set_max_level(log_filter(log_level));

    let net_params = &safe_ref!(net_params).0;
    let mwallet = obj_str(net_params, "wallet");

    let mrpc = json_to_rpc_config(net_params);

    if mrpc.is_none() {
        println!("Couldn't parse rpc json in GDKRPC_connect: {:#?}", net_params);
        return GA_ERROR;
    }
    let rpc = mrpc.unwrap();

    println!("Connecting to {} socks5({:#?})", rpc.url, rpc.socks5);
    let mclient = Network::connect(&rpc, mwallet);

    if let Err(msg) = mclient {
        println!("Error connecting to rpc: {}", msg);
        return GA_RECONNECT;
    }

    let client = mclient.unwrap();
    let mcount = client.get_block_count();

    if let Err(msg) = mcount {
        println!("Error establishing connection to rpc: {}", msg);
        return GA_RECONNECT;
    }

    sess.rpc_cfg = Some(rpc);

    println!("Client: {:#?}", client);
    println!("RpcConfig: {:#?}", sess.rpc_cfg);

    debug!("GA_connect() {:?}", sess);

    GA_OK
}

#[no_mangle]
pub extern "C" fn GDKRPC_disconnect(sess: *mut GDKRPC_session) -> i32 {
    let sess = safe_mut_ref!(sess);

    sess.rpc_cfg = None;
    if let Some(wallet) = sess.wallet.take() {
        tryit!(wallet.logout());
    }
    debug!("GA_disconnect() {:?}", sess);
    GA_OK
}

#[no_mangle]
pub extern "C" fn GDKRPC_register_user(
    sess: *mut GDKRPC_session,
    _hw_device: *const GDKRPC_json,
    mnemonic: *const c_char,
    ret: *mut *const GA_auth_handler,
) -> i32 {
    let sess = safe_mut_ref!(sess);
    let mnemonic = read_str(mnemonic);

    debug!("GA_register_user({:?}) {:?}", mnemonic, sess);
    tryit!(sess.rpc_cfg.as_ref().or_err("session not connected"));
    // sess.wallet = Some(tryit!(Wallet::register(network, &mnemonic)));

    ok!(ret, GA_auth_handler::success())
}

#[no_mangle]
pub extern "C" fn GDKRPC_login(
    sess: *mut GDKRPC_session,
    _hw_device: *const GDKRPC_json,
    mnemonic: *const c_char,
    password: *const c_char,
) -> i32 {
    let sess = safe_mut_ref!(sess);

    let mnemonic = read_str(mnemonic);

    if !read_str(password).is_empty() {
        println!("password-encrypted mnemonics are unsupported");
        return GA_ERROR;
    }

    if let Some(ref wallet) = sess.wallet {
        if wallet.mnemonic() != mnemonic {
            println!("user called login but was already logged-in");
            return GA_ERROR;
        } else {
            return GA_OK;
        }
    }

    if sess.rpc_cfg.is_none() {
        println!("Could not login. Not connected.");
        return GA_RECONNECT;
    }

    let rpc_cfg = sess.rpc_cfg.as_ref().unwrap();
    let mwallet = Wallet::login(&sess.networks, &rpc_cfg, &mnemonic, None);
    if let Err(msg) = mwallet {
        println!("Could not login: {}", msg);
        return GA_NOT_AUTHORIZED;
    }

    sess.wallet = Some(mwallet.unwrap());
    // let wallet = tryit!(sess.wallet.as_ref().or_err("session not connected"));

    tryit!(sess.hello());

    GA_OK
}

//
// Subaccounts
//

#[no_mangle]
pub extern "C" fn GDKRPC_get_subaccounts(
    sess: *const GDKRPC_session,
    ret: *mut *const GDKRPC_json,
) -> i32 {
    let sess = safe_ref!(sess);

    let wallet = tryit!(sess.wallet().or_err("no loaded wallet"));
    let account = tryit!(wallet.get_account());

    ok_json!(ret, vec![account])
}

#[no_mangle]
pub extern "C" fn GDKRPC_get_subaccount(
    sess: *const GDKRPC_session,
    _index: u32, // Ignored atm
    ret: *mut *const GDKRPC_json,
) -> i32 {
    let sess = safe_ref!(sess);

    let wallet = tryit!(sess.wallet().or_err("no loaded wallet"));
    let account = tryit!(wallet.get_account());

    ok_json!(ret, account)
}

//
// Transactions & Coins
//

#[no_mangle]
pub extern "C" fn GDKRPC_get_transactions(
    sess: *const GDKRPC_session,
    details: *const GDKRPC_json,
    ret: *mut *const GDKRPC_json,
) -> i32 {
    let sess = safe_ref!(sess);

    let details = &unsafe { &*details }.0;

    let wallet = tryit!(sess.wallet().or_err("no loaded wallet"));
    let txs = tryit!(wallet.get_transactions(&details));

    ok_json!(ret, txs)
}

#[no_mangle]
pub extern "C" fn GDKRPC_get_transaction_details(
    sess: *const GDKRPC_session,
    txid: *const c_char,
    ret: *mut *const GDKRPC_json,
) -> i32 {
    let sess = safe_ref!(sess);
    let txid = read_str(txid);

    let wallet = tryit!(sess.wallet().or_err("no loaded wallet"));
    let tx = tryit!(wallet.get_transaction(&txid));

    ok_json!(ret, tx)
}

#[no_mangle]
pub extern "C" fn GDKRPC_get_balance(
    sess: *const GDKRPC_session,
    details: *const GDKRPC_json,
    ret: *mut *const GDKRPC_json,
) -> i32 {
    let sess = safe_ref!(sess);

    let details = &unsafe { &*details }.0;

    let wallet = tryit!(sess.wallet().or_err("no loaded wallet"));
    let balance = tryit!(wallet.get_balance(&details));

    debug!("get_balance: {:?}", balance);

    ok_json!(ret, balance)
}

#[no_mangle]
pub extern "C" fn GDKRPC_set_transaction_memo(
    sess: *const GDKRPC_session,
    txid: *const c_char,
    memo: *const c_char,
    memo_type: u32,
) -> i32 {
    let sess = safe_ref!(sess);

    if memo_type != GA_MEMO_USER {
        warn!("unsupported memo type");
        return GA_ERROR;
    }

    let wallet = tryit!(sess.wallet().or_err("no loaded wallet"));

    let txid = read_str(txid);
    let memo = read_str(memo);

    tryit!(wallet.set_tx_memo(&txid, &memo[..]));

    GA_OK
}

//
// Creating transactions
//

#[no_mangle]
pub extern "C" fn GDKRPC_create_transaction(
    sess: *const GDKRPC_session,
    details: *const GDKRPC_json,
    ret: *mut *const GDKRPC_json,
) -> i32 {
    let sess = safe_ref!(sess);
    let details = &unsafe { &*details }.0;

    debug!("GA_create_transaction() {:?}", details);

    let wallet = tryit!(sess.wallet().or_err("no loaded wallet"));

    let res = json!({
        "addressees": &details["addressees"],
        "is_sweep": false,
        "memo": "",
        "subaccount": 0,
        "change_subaccount": 0,
        "fee": 100, // FIXME
        "satoshi": 500, // FIXME
    });

    let tx_unsigned = match wallet.create_transaction(&details) {
        Err(err) => {
            // errors are returned as a GA_OK with "error" in the returned object
            debug!("GA_create_transaction error: {:?}", err);
            return ok_json!(
                ret,
                extend(
                    res,
                    json!({
                        "error": err.to_gdk_code(),
                        "error_msg": err.to_string(),
                    })
                )
                .unwrap()
            );
        }
        Ok(x) => x,
    };

    debug!("GA_create_transaction() tx_unsigned {}", tx_unsigned);

    ok_json!(ret, extend(res, json!({ "error": "", "hex": tx_unsigned })).unwrap())
}

#[no_mangle]
pub extern "C" fn GDKRPC_sign_transaction(
    sess: *mut GDKRPC_session,
    tx_detail_unsigned: *mut GDKRPC_json,
    ret: *mut *const GDKRPC_json,
) -> i32 {
    let sess = safe_mut_ref!(sess);
    let tx_detail_unsigned = &mut safe_mut_ref!(tx_detail_unsigned).0;

    debug!("GA_sign_transaction() {:?}", tx_detail_unsigned);

    let wallet = tryit!(sess.wallet_mut().or_err("no loaded wallet"));
    let tx_signed = tryit!(wallet.sign_transaction(&tx_detail_unsigned));

    debug!("GA_sign_transaction() {:?}", tx_signed);

    const NO_CHANGE_INDEX: u32 = 0xffffffff;

    tx_detail_unsigned["hex"] = json!(tx_signed);
    tx_detail_unsigned["change_index"] = json!({ "btc": NO_CHANGE_INDEX });

    ok_json!(ret, tx_detail_unsigned)
}

#[no_mangle]
pub extern "C" fn GDKRPC_send_transaction(
    sess: *const GDKRPC_session,
    tx_detail_signed: *const GDKRPC_json,
    ret: *mut *const GDKRPC_json,
) -> i32 {
    let sess = safe_ref!(sess);
    debug!("GDKRPC_send_transaction deref");
    let tx_detail_signed = &safe_ref!(tx_detail_signed).0;

    debug!("GDKRPC_send_transaction detail_signed: {:?}", tx_detail_signed);

    let wallet = tryit!(sess.wallet().or_err("no loaded wallet"));
    let txid = tryit!(wallet.send_transaction(&tx_detail_signed));

    ok_json!(ret, json!({ "error": "", "txid": txid }))
}

#[no_mangle]
pub extern "C" fn GDKRPC_broadcast_transaction(
    sess: *const GDKRPC_session,
    tx_hex: *const c_char,
    ret: *mut *const c_char,
) -> i32 {
    let sess = safe_ref!(sess);
    let tx_hex = read_str(tx_hex);

    let wallet = tryit!(sess.wallet().or_err("no loaded wallet"));
    let txid = tryit!(wallet.send_raw_transaction(&tx_hex));

    ok!(ret, make_str(txid))
}

//
// Addresses
//

#[no_mangle]
pub extern "C" fn GDKRPC_get_receive_address(
    sess: *const GDKRPC_session,
    addr_details: *const GDKRPC_json,
    ret: *mut *const GDKRPC_json,
) -> i32 {
    let sess = safe_ref!(sess);
    let addr_details = &unsafe { &*addr_details }.0;

    let wallet = tryit!(sess.wallet().or_err("no loaded wallet"));
    let address = tryit!(wallet.get_receive_address(&addr_details));

    ok_json!(ret, address)
}

//
// Mnemonic
//

#[no_mangle]
pub extern "C" fn GDKRPC_get_mnemonic_passphrase(
    sess: *const GDKRPC_session,
    _password: *const c_char,
    ret: *mut *const c_char,
) -> i32 {
    let sess = safe_ref!(sess);
    let wallet = tryit!(sess.wallet().or_err("no loaded wallet"));
    ok!(ret, make_str(wallet.mnemonic()))
}

//
// Auth handler
//

#[no_mangle]
pub extern "C" fn GDKRPC_auth_handler_get_status(
    auth_handler: *const GA_auth_handler,
    ret: *mut *const GDKRPC_json,
) -> i32 {
    let auth_handler = safe_ref!(auth_handler);
    let status = auth_handler.to_json();

    ok_json!(ret, status)
}

//
// Currency conversion & fees
//

#[no_mangle]
pub extern "C" fn GDKRPC_get_available_currencies(
    sess: *const GDKRPC_session,
    ret: *mut *const GDKRPC_json,
) -> i32 {
    let sess = safe_ref!(sess);
    let wallet = tryit!(sess.wallet().or_err("no loaded wallet"));
    let currencies = wallet.get_available_currencies();

    ok_json!(ret, currencies)
}

#[no_mangle]
pub extern "C" fn GDKRPC_convert_amount(
    sess: *const GDKRPC_session,
    value_details: *const GDKRPC_json,
    ret: *mut *const GDKRPC_json,
) -> i32 {
    let sess = safe_ref!(sess);
    let value_details = &unsafe { &*value_details }.0;

    debug!("GA_convert_amount() {:?}", value_details);

    let wallet = tryit!(sess.wallet().or_err("no loaded wallet"));
    let units = tryit!(wallet.convert_amount(&value_details));

    debug!("GA_convert_amount() result: {:?}", units);

    ok_json!(ret, units)
}
#[no_mangle]
pub extern "C" fn GDKRPC_get_fee_estimates(
    sess: *const GDKRPC_session,
    ret: *mut *const GDKRPC_json,
) -> i32 {
    let sess = safe_ref!(sess);
    let wallet = tryit!(sess.wallet().or_err("no loaded wallet"));
    let estimates = tryit!(wallet.get_fee_estimates().or_err("fee estimates unavailable"));

    ok_json!(ret, json!({ "fees": estimates }))
}

//
// Push notifications
//

#[no_mangle]
pub extern "C" fn GDKRPC_set_notification_handler(
    sess: *mut GDKRPC_session,
    handler: extern "C" fn(*const libc::c_void, *const GDKRPC_json),
    self_context: *const libc::c_void,
) -> i32 {
    let sess = safe_mut_ref!(sess);
    sess.notify = Some((handler, self_context));

    println!("set notification handler");

    GA_OK
}

//
// Settings
//

#[no_mangle]
pub extern "C" fn GDKRPC_get_settings(
    sess: *const GDKRPC_session,
    ret: *mut *const GDKRPC_json,
) -> i32 {
    let sess = safe_ref!(sess);
    ok_json!(ret, json!(sess.settings))
}

#[no_mangle]
pub extern "C" fn GDKRPC_change_settings(
    sess: *mut GDKRPC_session,
    settings: *const GDKRPC_json,
    ret: *mut *const GA_auth_handler,
) -> i32 {
    let sess = safe_mut_ref!(sess);
    let new_settings = &unsafe { &*settings }.0;

    // XXX should we allow patching just some setting fields instead of replacing it?
    sess.settings = tryit!(from_value(new_settings.clone()));

    ok!(ret, GA_auth_handler::success())
}

//
// JSON utilities
//

#[no_mangle]
pub extern "C" fn GDKRPC_convert_json_to_string(
    json: *const GDKRPC_json,
    ret: *mut *const c_char,
) -> i32 {
    let json = &unsafe { &*json }.0;
    let res = json.to_string();
    ok!(ret, make_str(res))
}

#[no_mangle]
pub extern "C" fn GDKRPC_convert_string_to_json(
    jstr: *const c_char,
    ret: *mut *const GDKRPC_json,
) -> i32 {
    let jstr = read_str(jstr);
    let json: Value = tryit!(serde_json::from_str(&jstr));
    ok_json!(ret, json)
}

#[no_mangle]
pub extern "C" fn GDKRPC_convert_json_value_to_string(
    json: *const GDKRPC_json,
    path: *const c_char,
    ret: *mut *const c_char,
) -> i32 {
    let json = &unsafe { &*json }.0;
    let path = read_str(path);
    let res = tryit!(json[path].as_str().req());
    ok!(ret, make_str(res.to_string()))
}

#[no_mangle]
pub extern "C" fn GDKRPC_convert_json_value_to_uint32(
    json: *const GDKRPC_json,
    path: *const c_char,
    ret: *mut u32,
) -> i32 {
    let json = &unsafe { &*json }.0;
    let path = read_str(path);
    let res = tryit!(json[path].as_u64().req()) as u32;
    ok!(ret, res)
}

#[no_mangle]
pub extern "C" fn GDKRPC_convert_json_value_to_uint64(
    json: *const GDKRPC_json,
    path: *const c_char,
    ret: *mut u64,
) -> i32 {
    let json = &unsafe { &*json }.0;
    let path = read_str(path);
    let res = tryit!(json[path].as_u64().req());
    ok!(ret, res)
}

#[no_mangle]
pub extern "C" fn GDKRPC_convert_json_value_to_json(
    json: *const GDKRPC_json,
    path: *const c_char,
    ret: *mut *const GDKRPC_json,
) -> i32 {
    let json = &unsafe { &*json }.0;
    let path = read_str(path);
    let jstr = tryit!(json[path].as_str().req());
    let res: Value = tryit!(serde_json::from_str(jstr));
    ok_json!(ret, res)
}

#[no_mangle]
pub extern "C" fn GDKRPC_destroy_json(ptr: *mut GDKRPC_json) -> i32 {
    debug!("GA_destroy_json({:?})", ptr);
    // TODO make sure this works
    unsafe {
        drop(&*ptr);
    }
    GA_OK
}

#[no_mangle]
pub extern "C" fn GDKRPC_destroy_string(ptr: *mut c_char) -> i32 {
    unsafe {
        // retake pointer and drop
        let _ = CString::from_raw(ptr);
    }
    GA_OK
}

//
// Unimplemented, but gracefully degrades
//

#[no_mangle]
pub extern "C" fn GDKRPC_get_twofactor_config(
    // TODO: move in the cpp since it's hardcoded?
    _sess: *const GDKRPC_session,
    ret: *mut *const GDKRPC_json,
) -> i32 {
    // 2FA is always off
    ok_json!(
        ret,
        json!({
            "any_enabled":false,
            "all_methods":[],
            "enabled_methods":[],
            "email":{"confirmed":false,"data":"","enabled":false},
            "limits":{"bits":"0.00","btc":"0.00000000","fiat":"0.00","fiat_currency":"USD","fiat_rate":"0","is_fiat":false,"mbtc":"0.00000","satoshi":0,"ubtc":"0.00"},
            "twofactor_reset":{"days_remaining":-1,"is_active":false,"is_disputed":false},
        })
    )
}

#[no_mangle]
pub extern "C" fn GDKRPC_set_pin(
    _sess: *const GDKRPC_session,
    mnemonic: *const c_char,
    _pin: *const c_char,
    device_id: *const c_char,
    ret: *mut *const GDKRPC_json,
) -> i32 {
    let mnemonic = read_str(mnemonic);
    let device_id = read_str(device_id);
    let mnemonic_hex = hex::encode(&tryit!(wally::bip39_mnemonic_to_bytes(&mnemonic)));

    // FIXME setting a PIN does not actually do anything, just a successful no-op
    ok_json!(
        ret,
        json!({
            "encrypted_data": mnemonic_hex,
            "salt": "IA==",
            "pin_identifier": device_id,
            "__unencrypted": true
        })
    )
}
//
// Unimplemented and GA_ERROR's
//

#[no_mangle]
pub extern "C" fn GDKRPC_connect_with_proxy(
    _sess: *const GDKRPC_session,
    _network: *const c_char,
    _proxy_uri: *const c_char,
    _use_tor: u32,
    _log_level: u32,
) -> i32 {
    GA_ERROR
}

#[no_mangle]
pub extern "C" fn GDKRPC_get_unspent_outputs(
    _sess: *const GDKRPC_session,
    _details: *const GDKRPC_json,
    _ret: *mut *const GDKRPC_json,
) -> i32 {
    GA_ERROR
}

#[no_mangle]
pub extern "C" fn GDKRPC_get_unspent_outputs_for_private_key(
    _sess: *const GDKRPC_session,
    _private_key: *const c_char,
    _password: *const c_char,
    _unused: u32,
    _ret: *mut *const GDKRPC_json,
) -> i32 {
    GA_ERROR
}

#[no_mangle]
pub extern "C" fn GDKRPC_send_nlocktimes(_sess: *const GDKRPC_session) -> i32 {
    GA_ERROR
}

#[no_mangle]
pub extern "C" fn GDKRPC_register_network(
    _name: *const c_char,
    _network_details: *const GDKRPC_json,
) -> i32 {
    // let json : Value = safe_ref!(_network_details).0;
    // let name : String = read_str(_name);
    // let mnetwork : Result<Network, serde_json::Error> = serde_json::from_value(json);

    // if let Err(err) = mnetwork {
    //     println!("Error parsing network json in GDKRPC_register_network: {}", err);
    //     return GA_ERROR
    // }

    // let network = mnetwork.unwrap();

    // Network::list().insert(name, network);

    // GA_OK
    GA_ERROR
}

//
// Unit test helper methods
//

#[no_mangle]
pub extern "C" fn GDKRPC_test_tick(sess: *mut GDKRPC_session) -> i32 {
    debug!("GA_test_tick()");
    let sess = safe_mut_ref!(sess);
    tryit!(sess.tick());
    GA_OK
}