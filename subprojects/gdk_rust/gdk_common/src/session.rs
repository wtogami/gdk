use crate::model::{AddressResult, FeeEstimate, Subaccount, TxsResult};

// TODO: remove all json Values from our Session
use serde_json::Value;

pub trait Session<E> {
    // fn create_session(network: Network) -> Result<Self::Value, E>;
    fn destroy_session(&mut self) -> Result<(), E>;
    fn poll_session(&self) -> Result<(), E>;
    fn connect(&mut self, net_params: &Value) -> Result<(), E>;
    fn disconnect(&mut self) -> Result<(), E>;
    // fn register_user(&mut self, mnemonic: String) -> Result<(), E>;
    fn login(&mut self, mnemonic: String, password: Option<String>) -> Result<(), E>;
    fn get_subaccounts(&self) -> Result<Vec<Subaccount>, E>;
    fn get_subaccount(&self, index: u32, num_confs: u32) -> Result<Subaccount, E>;
    fn get_transactions(&self, details: &Value) -> Result<TxsResult, E>;
    fn get_transaction_details(&self, txid: &str) -> Result<Value, E>;
    fn get_balance(&self, num_confs: u32, subaccount: Option<u32>) -> Result<u64, E>;
    fn set_transaction_memo(&self, txid: &str, memo: &str, memo_type: u32) -> Result<(), E>;
    fn create_transaction(&self, details: &Value) -> Result<Value, E>;
    fn sign_transaction(&self, tx_detail_unsigned: &Value) -> Result<Value, E>;
    fn send_transaction(&self, tx_detail_signed: &Value) -> Result<String, E>;
    fn broadcast_transaction(&self, tx_hex: &str) -> Result<String, E>;
    fn get_receive_address(&self, addr_details: &Value) -> Result<AddressResult, E>;
    fn get_mnemonic_passphrase(&self, _password: &str) -> Result<String, E>;
    fn get_available_currencies(&self) -> Result<Value, E>;
    // fn convert_amount(&self, value_details: &Value) -> Result<Value, E>;
    fn get_fee_estimates(&mut self) -> Result<Vec<FeeEstimate>, E>;
    fn get_settings(&self) -> Result<Value, E>;
    fn change_settings(&mut self, settings: &Value) -> Result<(), E>;
}