use crate::be::{AssetId, BEOutPoint, BETransaction, BETransactions, UTXOInfo, Unblinded, Utxos};
use bitcoin::hashes::hex::{FromHex, ToHex};
use bitcoin::{Network, Script, Txid};
use core::mem::transmute;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::Error;
use bitcoin::hashes::core::fmt::Formatter;
use bitcoin::util::bip32::{ChildNumber, DerivationPath};
use chrono::{DateTime, NaiveDateTime, Utc};
use serde_json::Value;
use std::convert::{TryFrom, TryInto};
use std::fmt;
use std::fmt::Display;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug)]
#[repr(C)]
pub struct GDKRUST_json(pub serde_json::Value);

impl GDKRUST_json {
    pub fn new(data: serde_json::Value) -> *const GDKRUST_json {
        unsafe { transmute(Box::new(GDKRUST_json(data))) }
    }
}

pub type Balances = HashMap<String, i64>;

// =========== v exchange rate stuff v ===========

// TODO use these types from bitcoin-exchange-rates lib once it's in there

#[derive(Debug, Clone, PartialEq)]
pub struct ExchangeRate {
    pub currency: String,
    pub rate: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExchangeRateError {
    pub message: String,
    pub error: ExchangeRateErrorType,
}

impl Display for ExchangeRateError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

impl Display for ExchangeRateErrorType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExchangeRateOk {
    NoBackends, // this probably should't be a hard error,
    // so we label it an Ok result
    RateOk(ExchangeRate),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum ExchangeRateErrorType {
    FetchError,
    ParseError,
}

pub type ExchangeRateRes = Result<ExchangeRateOk, ExchangeRateError>;

impl ExchangeRateOk {
    pub fn ok(currency: String, rate: f64) -> ExchangeRateOk {
        ExchangeRateOk::RateOk(ExchangeRate {
            currency,
            rate,
        })
    }

    pub fn no_backends() -> ExchangeRateOk {
        ExchangeRateOk::NoBackends
    }
}

// =========== ^ exchange rate stuff ^ ===========

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct AddressAmount {
    pub address: String, // could be bitcoin or elements
    pub satoshi: u64,
    pub asset_tag: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct BlockNotification {
    //pub block_hash: bitcoin::BlockHash,
    pub block_height: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TransactionNotification {
    pub transaction_hash: bitcoin::Txid,
}

#[derive(Debug)]
pub enum Notification {
    Block(BlockNotification),
    Transaction(TransactionNotification),
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct CreateTransaction {
    pub addressees: Vec<AddressAmount>,
    pub fee_rate: Option<u64>, // in satoshi/kbyte
    pub subaccount: Option<u32>,
    pub send_all: Option<bool>,
    #[serde(default)]
    pub previous_transaction: HashMap<String, Value>,
    pub memo: Option<String>,
    pub utxos: Option<GetUnspentOutputs>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct GetTransactionsOpt {
    pub first: usize,
    pub count: usize,
    pub subaccount: usize,
    pub num_confs: Option<usize>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct SPVVerifyTx {
    pub txid: String,
    pub height: u32,
    pub path: String,
    pub network: crate::network::Network,
    pub encryption_key: String,
    pub tor_proxy: Option<String>,
    pub headers_to_download: Option<usize>, // defaults to 2016, useful to set for testing
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum SPVVerifyResult {
    InProgress,
    Verified,
    NotVerified,
    Disabled,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TransactionMeta {
    #[serde(flatten)]
    pub create_transaction: Option<CreateTransaction>,
    #[serde(rename = "transaction")]
    pub hex: String,
    pub txid: String,
    pub height: Option<u32>,
    pub timestamp: u32, // for confirmed tx is block time for unconfirmed is when created or when list_tx happens
    pub created_at: String, // yyyy-MM-dd HH:mm:ss of timestamp
    pub error: String,
    pub addressees_have_assets: bool,
    pub is_sweep: bool,
    pub satoshi: Balances,
    pub fee: u64,
    pub network: Option<Network>,
    #[serde(rename = "type")]
    pub type_: String, // incoming or outgoing
    pub changes_used: Option<u32>,
    pub rbf_optin: bool,
    pub user_signed: bool,
    pub spv_verified: SPVVerifyResult,
}

impl From<BETransaction> for TransactionMeta {
    fn from(transaction: BETransaction) -> Self {
        let txid = transaction.txid().to_string();
        let hex = hex::encode(&transaction.serialize());
        let timestamp = now();
        let rbf_optin = transaction.rbf_optin();

        TransactionMeta {
            create_transaction: None,
            height: None,
            created_at: format(timestamp),
            timestamp,
            txid,
            hex,
            error: "".to_string(),
            addressees_have_assets: false,
            is_sweep: false,
            satoshi: HashMap::new(),
            fee: 0,
            network: None,
            type_: "unknown".to_string(),
            changes_used: None,
            user_signed: false,
            spv_verified: SPVVerifyResult::InProgress,
            rbf_optin,
        }
    }
}

impl TransactionMeta {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        transaction: BETransaction,
        height: Option<u32>,
        timestamp: Option<u32>,
        satoshi: Balances,
        fee: u64,
        network: Network,
        type_: String,
        create_transaction: CreateTransaction,
        user_signed: bool,
        spv_verified: SPVVerifyResult,
    ) -> Self {
        let mut wgtx: TransactionMeta = transaction.into();
        let timestamp = timestamp.unwrap_or_else(now);
        let created_at = format(timestamp);

        wgtx.create_transaction = Some(create_transaction);
        wgtx.height = height;
        wgtx.timestamp = timestamp;
        wgtx.created_at = created_at;
        wgtx.satoshi = satoshi;
        wgtx.network = Some(network);
        wgtx.fee = fee;
        wgtx.type_ = type_;
        wgtx.user_signed = user_signed;
        wgtx.spv_verified = spv_verified;
        wgtx
    }

    fn transaction(&self, network_id: crate::network::NetworkId) -> Result<BETransaction, Error> {
        Ok(BETransaction::from_hex(&self.hex, network_id)?)
    }

    pub fn make_txlist_item(
        &self,
        all_txs: &BETransactions,
        all_unblinded: &HashMap<elements::OutPoint, Unblinded>,
        network_id: crate::network::NetworkId,
    ) -> Result<TxListItem, Error> {
        let type_ = self.type_.clone();
        let len = self.hex.len() / 2;
        let fee_rate = (self.fee as f64 / len as f64) as u64;
        let addressees = self
            .create_transaction
            .as_ref()
            .unwrap()
            .addressees
            .iter()
            .map(|e| e.address.clone())
            .collect();

        let transaction = self.transaction(network_id)?;
        let inputs = transaction
            .previous_outputs()
            .iter()
            .enumerate()
            .map(|(vin, i)| {
                let mut a = AddressIO::default();
                a.pt_idx = vin as u32;
                a.satoshi = all_txs.get_previous_output_value(i, all_unblinded).unwrap_or_default();
                if let BEOutPoint::Elements(outpoint) = i {
                    a.asset_id = all_txs
                        .get_previous_output_asset_hex(*outpoint, all_unblinded)
                        .unwrap_or_default();
                    a.assetblinder = all_txs
                        .get_previous_output_assetblinder_hex(*outpoint, all_unblinded)
                        .unwrap_or_default();
                    a.amountblinder = all_txs
                        .get_previous_output_amountblinder_hex(*outpoint, all_unblinded)
                        .unwrap_or_default();
                }
                a
            })
            .collect();

        let mut outputs: Vec<AddressIO> = vec![];
        for vout in 0..transaction.output_len() as u32 {
            let mut a = AddressIO::default();
            a.pt_idx = vout;
            a.satoshi = transaction.output_value(vout, all_unblinded).unwrap_or_default();
            if let BETransaction::Elements(_) = transaction {
                a.asset_id = transaction.output_asset_hex(vout, all_unblinded).unwrap_or_default();
                a.assetblinder =
                    transaction.output_assetblinder_hex(vout, all_unblinded).unwrap_or_default();
                a.amountblinder =
                    transaction.output_amountblinder_hex(vout, all_unblinded).unwrap_or_default();
            }
            outputs.push(a);
        }

        Ok(TxListItem {
            block_height: self.height.unwrap_or_default(),
            created_at: self.created_at.clone(),
            type_,
            memo: self
                .create_transaction
                .as_ref()
                .and_then(|c| c.memo.clone())
                .unwrap_or("".to_string()),
            txhash: self.txid.clone(),
            transaction_size: len,
            transaction: self.hex.clone(), // FIXME
            satoshi: self.satoshi.clone(),
            rbf_optin: self.rbf_optin, // TODO: TransactionMeta -> TxListItem rbf_optin
            cap_cpfp: false,           // TODO: TransactionMeta -> TxListItem cap_cpfp
            can_rbf: false,            // TODO: TransactionMeta -> TxListItem can_rbf
            has_payment_request: false, // TODO: TransactionMeta -> TxListItem has_payment_request
            server_signed: false,      // TODO: TransactionMeta -> TxListItem server_signed
            user_signed: self.user_signed,
            spv_verified: self.spv_verified.to_string(),
            instant: false,
            fee: self.fee,
            fee_rate,
            addressees, // notice the extra "e" -- its intentional
            inputs,
            outputs,
            transaction_vsize: len,  //TODO
            transaction_weight: len, //TODO
        })
    }
}

#[derive(Debug, Clone)]
pub struct AddressIO {
    pub address: String,
    pub address_type: bitcoin::util::address::AddressType,
    pub addressee: String,
    pub is_output: String,
    pub is_relevant: String,
    pub is_spent: String,
    pub pointer: u32, // child_number in bip32 terminology
    pub pt_idx: u32,  // vout
    pub satoshi: u64,
    pub asset_id: String,
    pub assetblinder: String,
    pub amountblinder: String,
    pub script_type: u32,
    pub subaccount: u32,
    pub subtype: u32, // unused here, but used in gdk interface for CSV bucketing
}

impl Default for AddressIO {
    fn default() -> Self {
        AddressIO {
            address: "".into(),
            address_type: bitcoin::util::address::AddressType::P2sh,
            addressee: "".into(),
            asset_id: "".into(),
            is_output: "".into(),
            is_relevant: "".into(),
            is_spent: "".into(),
            pointer: 0,
            pt_idx: 0,
            satoshi: 0,
            script_type: 0,
            subaccount: 0,
            subtype: 0,
            assetblinder: "".into(),
            amountblinder: "".into(),
        }
    }
}

// TODO remove TxListItem, make TransactionMeta compatible and automatically serialized
#[derive(Debug, Clone)]
pub struct TxListItem {
    pub block_height: u32,
    pub created_at: String,
    pub type_: String,
    pub memo: String,
    pub txhash: String,
    pub transaction: String,
    pub satoshi: Balances,
    pub rbf_optin: bool,
    pub cap_cpfp: bool,
    pub can_rbf: bool,
    pub has_payment_request: bool,
    pub server_signed: bool,
    pub user_signed: bool,
    pub instant: bool,
    pub spv_verified: String,
    pub fee: u64,
    pub fee_rate: u64,
    pub addressees: Vec<String>, // receiver's addresses
    pub inputs: Vec<AddressIO>,  // tx.input.iter().map(format_gdk_input).collect(),
    pub outputs: Vec<AddressIO>, //tx.output.iter().map(format_gdk_output).collect(),
    pub transaction_size: usize,
    pub transaction_vsize: usize,
    pub transaction_weight: usize,
}

pub struct Subaccount {
    pub type_: String,
    pub name: String,
    pub has_transactions: bool,
    pub satoshi: Balances,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PinSetDetails {
    pub pin: String,
    pub mnemonic: String,
    pub device_id: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PinGetDetails {
    pub salt: String,
    pub encrypted_data: String,
    pub pin_identifier: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AddressPointer {
    pub address: String,
    pub pointer: u32, // child_number in bip32 terminology
}

// This one is simple enough to derive a serializer
#[derive(Serialize, Debug, Clone, Deserialize)]
pub struct FeeEstimate(pub u64);
pub struct TxsResult(pub Vec<TxListItem>);

/// Change to the model of Settings and Pricing structs could break old versions.
/// You can't remove fields, change fields type and if you add a new field, it must be Option<T>
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Settings {
    pub unit: String,
    pub required_num_blocks: u32,
    pub altimeout: u32,
    pub pricing: Pricing,
    pub sound: bool,
}

/// {"icons":true,"assets":false,"refresh":false}
#[derive(Serialize, Deserialize, Debug)]
pub struct RefreshAssets {
    pub icons: bool,
    pub assets: bool,
    pub refresh: bool,
}

impl RefreshAssets {
    pub fn new(icons: bool, assets: bool, refresh: bool) -> Self {
        RefreshAssets {
            icons,
            assets,
            refresh,
        }
    }
}

/// see comment for struct Settings
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Pricing {
    currency: String,
    exchange: String,
}

impl Default for Settings {
    fn default() -> Self {
        let pricing = Pricing {
            currency: "USD".to_string(),
            exchange: "BITFINEX".to_string(),
        };
        Settings {
            unit: "BTC".to_string(),
            required_num_blocks: 12,
            altimeout: 600,
            pricing,
            sound: false,
        }
    }
}

impl AddressAmount {
    pub fn asset(&self) -> Option<AssetId> {
        if let Some(asset_tag) = self.asset_tag.as_ref() {
            let vec = hex::decode(asset_tag).ok();
            if let Some(mut vec) = vec {
                vec.reverse();
                return (&vec[..]).try_into().ok();
            }
        }
        None
    }
}

fn now() -> u32 {
    let start = SystemTime::now();
    let since_the_epoch = start.duration_since(UNIX_EPOCH).expect("Time went backwards");
    since_the_epoch.as_secs() as u32
}

fn format(timestamp: u32) -> String {
    let dt = DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(timestamp as i64, 0), Utc);
    format!("{}", dt.format("%Y-%m-%d %H:%M:%S"))
}

impl SPVVerifyResult {
    pub fn as_i32(&self) -> i32 {
        match self {
            SPVVerifyResult::InProgress => 0,
            SPVVerifyResult::Verified => 1,
            SPVVerifyResult::NotVerified => 2,
            SPVVerifyResult::Disabled => 3,
        }
    }
}

impl Display for SPVVerifyResult {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            SPVVerifyResult::InProgress => write!(f, "in_progress"),
            SPVVerifyResult::Verified => write!(f, "verified"),
            SPVVerifyResult::NotVerified => write!(f, "not_verified"),
            SPVVerifyResult::Disabled => write!(f, "disabled"),
        }
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GetUnspentOutputs(pub HashMap<String, Vec<UnspentOutput>>);

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnspentOutput {
    pub address_type: String,
    pub block_height: u32,
    pub pointer: u32,
    pub pt_idx: u32,
    pub satoshi: u64,
    pub subaccount: u32,
    pub txhash: String,
    pub derivation_path: String,  // not present in gdk-cpp
    pub scriptpubkey_hex: String, // not present in gdk-cpp
}

impl UnspentOutput {
    pub fn new(outpoint: &BEOutPoint, info: &UTXOInfo) -> Self {
        let mut unspent_output = UnspentOutput::default();
        unspent_output.address_type = "p2shwpkh".to_string();
        unspent_output.satoshi = info.value;
        unspent_output.txhash = format!("{}", outpoint.txid());
        unspent_output.pt_idx = outpoint.vout();
        unspent_output.derivation_path = info.path.to_string();
        unspent_output.scriptpubkey_hex = info.script.to_hex();
        let childs: Vec<ChildNumber> = info.path.clone().into();
        if let Some(ChildNumber::Normal {
            index,
        }) = childs.last()
        {
            unspent_output.pointer = *index;
        }
        unspent_output.block_height = info.height.unwrap_or(0);
        unspent_output
    }
}

impl TryFrom<&GetUnspentOutputs> for Utxos {
    type Error = Error;

    fn try_from(unspent_outputs: &GetUnspentOutputs) -> Result<Self, Error> {
        let mut utxos = vec![];
        for (asset, v) in unspent_outputs.0.iter() {
            for e in v {
                let txid = Txid::from_hex(&e.txhash).unwrap(); // FIXME
                let outpoint = match &asset[..] {
                    "btc" => BEOutPoint::new_bitcoin(txid, e.pt_idx),
                    _ => BEOutPoint::new_elements(txid, e.pt_idx),
                };
                let script = Script::from(hex::decode(&e.scriptpubkey_hex)?);
                let height = match e.block_height {
                    0 => None,
                    n => Some(n),
                };
                let path = DerivationPath::from_str(&e.derivation_path)?;
                let utxo_info = UTXOInfo::new(asset.to_string(), e.satoshi, script, height, path);
                utxos.push((outpoint, utxo_info));
            }
        }
        Ok(utxos)
    }
}

#[cfg(test)]
mod test {
    use crate::model::GetUnspentOutputs;

    #[test]
    fn test_unspent() {
        let json_str = r#"{"btc": [{"address_type": "p2wsh", "block_height": 1806588, "pointer": 3509, "pt_idx": 1, "satoshi": 3650144, "subaccount": 0, "txhash": "08711d45d4867d7834b133a425da065b252eb6a9b206d57e2bbb226a344c5d13", "derivation_path": "m", "scriptpubkey_hex": "51"}, {"address_type": "p2wsh", "block_height": 1835681, "pointer": 3510, "pt_idx": 0, "satoshi": 5589415, "subaccount": 0, "txhash": "fbd00e5b9e8152c04214c72c791a78a65fdbab68b5c6164ff0d8b22a006c5221", "derivation_path": "m", "scriptpubkey_hex": "51"}, {"address_type": "p2wsh", "block_height": 1835821, "pointer": 3511, "pt_idx": 0, "satoshi": 568158, "subaccount": 0, "txhash": "e5b358fb8366960130b97794062718d7f4fbe721bf274f47493a19326099b811", "derivation_path": "m", "scriptpubkey_hex": "51"}]}"#;
        let json: GetUnspentOutputs = serde_json::from_str(json_str).unwrap();
        println!("{:#?}", json);
    }
}
