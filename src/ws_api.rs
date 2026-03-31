use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signature, Signer, SigningKey};
use hex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct RateLimit {
    pub rateLimitType: String,
    pub interval: String,
    pub intervalNum: i32,
    pub limit: i32,
    pub count: i32,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LoginResult {
    pub apiKey: String,
    pub authorizedSince: i64,
    pub connectedSince: i64,
    pub returnRateLimits: bool,
    pub serverTime: i64,
}

#[derive(Debug, Deserialize)]
pub struct LoginResponse {
    pub id: String,
    pub status: i32,
    pub result: Option<LoginResult>,
    pub rateLimits: Option<Vec<RateLimit>>,
    pub error: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct UserDataStreamResult {
    pub listenKey: String,
}

#[derive(Debug, Deserialize)]
pub struct UserDataStreamResponse {
    pub id: String,
    pub status: i32,
    pub result: Option<UserDataStreamResult>,
    pub rateLimits: Option<Vec<RateLimit>>,
    pub error: Option<serde_json::Value>,
}

pub fn parse_login_response(response: &str) -> Result<LoginResponse, String> {
    serde_json::from_str(response).map_err(|e| format!("JSON解析失败: {}", e))
}

pub fn parse_user_data_stream_response(response: &str) -> Result<UserDataStreamResponse, String> {
    serde_json::from_str(response).map_err(|e| format!("JSON解析失败: {}", e))
}

pub fn is_login_success(response: &str) -> bool {
    match parse_login_response(response) {
        Ok(resp) => resp.status == 200,
        Err(_) => false,
    }
}

pub fn check_response_id(response: &str, expected_id: &str) -> bool {
    match serde_json::from_str::<serde_json::Value>(response) {
        Ok(json) => {
            json.get("id")
                .and_then(|v| v.as_str())
                .map(|id| id == expected_id)
                .unwrap_or(false)
        }
        Err(_) => false,
    }
}

pub fn sign_message(timestamp: i64, api_key: &str, private_key_hex: &str) -> String {
    let message = format!("apiKey={}&timestamp={}", api_key, timestamp);

    let private_key_bytes = hex::decode(private_key_hex)
        .expect("私钥十六进制解码失败");
    
    let signing_key = SigningKey::from_bytes(
        private_key_bytes
            .as_slice()
            .try_into()
            .expect("私钥长度必须是32字节")
    );

    let signature = signing_key.sign(message.as_bytes());
    
    STANDARD.encode(signature.to_bytes())
}

pub fn create_auth_request(id: &str, api_key: &str, signature: &str, timestamp: i64) -> String {
    let json = serde_json::json!({
        "id": id,
        "method": "session.logon",
        "params": {
            "apiKey": api_key,
            "signature": signature,
            "timestamp": timestamp
        }
    });
    
    serde_json::to_string(&json).expect("JSON序列化失败")
}

pub fn login(id: &str, api_key: &str, private_key: &str, timestamp: i64) -> String {
    let signature = sign_message(timestamp, api_key, private_key);
    create_auth_request(id, api_key, &signature, timestamp)
}

pub fn create_user_data_stream_request(id: &str, api_key: &str) -> String {
    let json = serde_json::json!({
        "id": id,
        "method": "userDataStream.start",
        "params": {
            "apiKey": api_key
        }
    });
    
    serde_json::to_string(&json).expect("JSON序列化失败")
}

pub fn create_ping_user_data_stream_request(id: &str, api_key: &str) -> String {
    let json = serde_json::json!({
        "id": id,
        "method": "userDataStream.ping",
        "params": {
            "apiKey": api_key
        }
    });
    
    serde_json::to_string(&json).expect("JSON序列化失败")
}

pub fn parse_ping_response(response: &str) -> Result<UserDataStreamResponse, String> {
    serde_json::from_str(response).map_err(|e| format!("JSON解析失败: {}", e))
}

#[derive(Debug, Deserialize)]
pub struct BalanceItem {
    #[serde(rename = "accountAlias")]
    pub account_alias: String,
    pub asset: String,
    pub balance: String,
    #[serde(rename = "crossWalletBalance")]
    pub cross_wallet_balance: String,
    #[serde(rename = "crossUnPnl")]
    pub cross_un_pnl: String,
    #[serde(rename = "availableBalance")]
    pub available_balance: String,
    #[serde(rename = "maxWithdrawAmount")]
    pub max_withdraw_amount: String,
    #[serde(rename = "marginAvailable")]
    pub margin_available: bool,
    #[serde(rename = "updateTime")]
    pub update_time: i64,
}

#[derive(Debug, Deserialize)]
pub struct AccountBalanceResponse {
    pub id: String,
    pub status: i32,
    pub result: Option<Vec<BalanceItem>>,
    pub rateLimits: Option<Vec<RateLimit>>,
    pub error: Option<serde_json::Value>,
}

pub fn create_account_balance_request(id: &str, api_key: &str, private_key: &str, timestamp: i64) -> String {
    let signature = sign_message(timestamp, api_key, private_key);
    let json = serde_json::json!({
        "id": id,
        "method": "v2/account.balance",
        "params": {
            "apiKey": api_key,
            "timestamp": timestamp,
            "signature": signature
        }
    });
    
    serde_json::to_string(&json).expect("JSON序列化失败")
}

pub fn parse_account_balance_response(response: &str) -> Result<AccountBalanceResponse, String> {
    serde_json::from_str(response).map_err(|e| format!("JSON解析失败: {}", e))
}

pub fn create_market_subscribe_request(id: u64, streams: Vec<&str>) -> String {
    let json = serde_json::json!({
        "method": "SUBSCRIBE",
        "params": streams,
        "id": id
    });
    
    serde_json::to_string(&json).expect("JSON序列化失败")
}
