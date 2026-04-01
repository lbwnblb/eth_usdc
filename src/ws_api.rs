use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signer, SigningKey};
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

#[derive(Debug, Deserialize)]
pub struct BookTickerData {
    #[serde(rename = "e")]
    pub event_type: String,
    #[serde(rename = "u")]
    pub update_id: u64,
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "b")]
    pub best_bid_price: String,
    #[serde(rename = "B")]
    pub best_bid_qty: String,
    #[serde(rename = "a")]
    pub best_ask_price: String,
    #[serde(rename = "A")]
    pub best_ask_qty: String,
    #[serde(rename = "T")]
    pub transaction_time: u64,
    #[serde(rename = "E")]
    pub event_time: u64,
}

#[derive(Debug, Deserialize)]
pub struct BookTickerStream {
    pub stream: String,
    pub data: BookTickerData,
}

pub fn parse_book_ticker(response: &str) -> Result<BookTickerStream, String> {
    serde_json::from_str(response).map_err(|e| format!("BookTicker JSON解析失败: {}", e))
}

pub fn create_order_request(
    id: &str,
    symbol: &str,
    side: &str,
    order_type: &str,
    quantity: f64,
    price: Option<f64>,
    time_in_force: Option<&str>,
    position_side: Option<&str>,
    new_client_order_id: Option<&str>,
    timestamp: i64,
) -> String {
    let mut params = serde_json::json!({
        "symbol": symbol,
        "side": side,
        "type": order_type,
        "quantity": quantity,
        "timestamp": timestamp
    });

    if let Some(p) = price {
        params["price"] = p.into();
    }

    if let Some(tif) = time_in_force {
        params["timeInForce"] = tif.into();
    }

    if let Some(ps) = position_side {
        params["positionSide"] = ps.into();
    }

    if let Some(ncoid) = new_client_order_id {
        params["newClientOrderId"] = ncoid.into();
    }

    let json = serde_json::json!({
        "id": id,
        "method": "order.place",
        "params": params
    });

    serde_json::to_string(&json).expect("JSON序列化失败")
}

#[derive(Debug, Deserialize)]
pub struct Order {
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "c")]
    pub client_order_id: String,
    #[serde(rename = "S")]
    pub side: String,
    #[serde(rename = "o")]
    pub order_type: String,
    #[serde(rename = "f")]
    pub time_in_force: String,
    #[serde(rename = "q")]
    pub quantity: String,
    #[serde(rename = "p")]
    pub price: String,
    #[serde(rename = "ap")]
    pub avg_price: String,
    #[serde(rename = "sp")]
    pub stop_price: String,
    #[serde(rename = "x")]
    pub execution_type: String,
    #[serde(rename = "X")]
    pub order_status: String,
    #[serde(rename = "i")]
    pub order_id: u64,
    #[serde(rename = "l")]
    pub last_filled_qty: String,
    #[serde(rename = "z")]
    pub filled_accumulated_qty: String,
    #[serde(rename = "L")]
    pub last_filled_price: String,
    #[serde(rename = "n")]
    pub commission: String,
    #[serde(rename = "N")]
    pub commission_asset: Option<String>,
    #[serde(rename = "T")]
    pub transaction_time: u64,
    #[serde(rename = "t")]
    pub trade_id: u64,
    #[serde(rename = "b")]
    pub bids_notional: String,
    #[serde(rename = "a")]
    pub asks_notional: String,
    #[serde(rename = "m")]
    pub is_maker: bool,
    #[serde(rename = "R")]
    pub is_reduce_only: bool,
    #[serde(rename = "wt")]
    pub working_type: String,
    #[serde(rename = "ot")]
    pub original_order_type: String,
    #[serde(rename = "ps")]
    pub position_side: String,
    #[serde(rename = "cp")]
    pub close_all: bool,
    #[serde(rename = "rp")]
    pub realized_profit: String,
    #[serde(rename = "pP")]
    pub price_protect: bool,
    #[serde(rename = "si")]
    pub ignore1: i64,
    #[serde(rename = "ss")]
    pub ignore2: i64,
    #[serde(rename = "V")]
    pub self_trade_prevention_mode: String,
    #[serde(rename = "pm")]
    pub price_match: String,
    #[serde(rename = "gtd")]
    pub gtd_order_auto_cancel_time: u64,
    #[serde(rename = "er")]
    pub good_till_date: String,
}

#[derive(Debug, Deserialize)]
pub struct OrderTradeUpdateData {
    #[serde(rename = "e")]
    pub event_type: String,
    #[serde(rename = "T")]
    pub transaction_time: u64,
    #[serde(rename = "E")]
    pub event_time: u64,
    #[serde(rename = "o")]
    pub order: Order,
}

#[derive(Debug, Deserialize)]
pub struct OrderTradeUpdateStream {
    pub stream: String,
    pub data: OrderTradeUpdateData,
}

#[derive(Debug, Deserialize)]
pub struct TradeLiteData {
    #[serde(rename = "e")]
    pub event_type: String,
    #[serde(rename = "E")]
    pub event_time: u64,
    #[serde(rename = "T")]
    pub transaction_time: u64,
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "q")]
    pub quantity: String,
    #[serde(rename = "p")]
    pub price: String,
    #[serde(rename = "m")]
    pub is_maker: bool,
    #[serde(rename = "c")]
    pub client_order_id: String,
    #[serde(rename = "S")]
    pub side: String,
    #[serde(rename = "L")]
    pub last_filled_price: String,
    #[serde(rename = "l")]
    pub last_filled_qty: String,
    #[serde(rename = "t")]
    pub trade_id: u64,
    #[serde(rename = "i")]
    pub order_id: u64,
}

#[derive(Debug, Deserialize)]
pub struct TradeLiteStream {
    pub stream: String,
    pub data: TradeLiteData,
}

#[derive(Debug, Deserialize)]
pub struct AccountBalance {
    #[serde(rename = "a")]
    pub asset: String,
    #[serde(rename = "wb")]
    pub wallet_balance: String,
    #[serde(rename = "cw")]
    pub cross_wallet_balance: String,
    #[serde(rename = "bc")]
    pub balance_change: String,
}

#[derive(Debug, Deserialize)]
pub struct Position {
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "pa")]
    pub position_amount: String,
    #[serde(rename = "ep")]
    pub entry_price: String,
    #[serde(rename = "cr")]
    pub accumulated_realized: String,
    #[serde(rename = "up")]
    pub unrealized_pnl: String,
    #[serde(rename = "mt")]
    pub margin_type: String,
    #[serde(rename = "iw")]
    pub isolated_wallet: String,
    #[serde(rename = "ps")]
    pub position_side: String,
    #[serde(rename = "ma")]
    pub margin_asset: String,
    #[serde(rename = "bep")]
    pub break_even_price: String,
}

#[derive(Debug, Deserialize)]
pub struct AccountUpdateData {
    #[serde(rename = "e")]
    pub event_type: String,
    #[serde(rename = "T")]
    pub transaction_time: u64,
    #[serde(rename = "E")]
    pub event_time: u64,
    #[serde(rename = "a")]
    pub account: AccountInfo,
}

#[derive(Debug, Deserialize)]
pub struct AccountInfo {
    #[serde(rename = "B")]
    pub balances: Vec<AccountBalance>,
    #[serde(rename = "P")]
    pub positions: Vec<Position>,
    #[serde(rename = "m")]
    pub reason: String,
}

#[derive(Debug, Deserialize)]
pub struct AccountUpdateStream {
    pub stream: String,
    pub data: AccountUpdateData,
}

#[derive(Debug)]
pub enum UserDataStreamEvent {
    OrderTradeUpdate(OrderTradeUpdateStream),
    TradeLite(TradeLiteStream),
    AccountUpdate(AccountUpdateStream),
    Unknown(serde_json::Value),
}

#[derive(Debug, Deserialize)]
pub struct GenericResponse {
    pub id: String,
    pub status: i32,
    pub result: Option<serde_json::Value>,
    pub rateLimits: Option<Vec<RateLimit>>,
    pub error: Option<serde_json::Value>,
}

pub fn parse_generic_response(response: &str) -> Result<GenericResponse, String> {
    serde_json::from_str(response).map_err(|e| format!("JSON解析失败: {}", e))
}

pub fn parse_user_data_stream(response: &str) -> Result<UserDataStreamEvent, String> {
    let json: serde_json::Value = serde_json::from_str(response)
        .map_err(|e| format!("JSON解析失败: {}", e))?;
    
    if let Some(data) = json.get("data") {
        if let Some(event_type) = data.get("e").and_then(|e| e.as_str()) {
            match event_type {
                "ORDER_TRADE_UPDATE" => {
                    let stream: OrderTradeUpdateStream = serde_json::from_str(response)
                        .map_err(|e| format!("ORDER_TRADE_UPDATE解析失败: {}", e))?;
                    return Ok(UserDataStreamEvent::OrderTradeUpdate(stream));
                }
                "TRADE_LITE" => {
                    let stream: TradeLiteStream = serde_json::from_str(response)
                        .map_err(|e| format!("TRADE_LITE解析失败: {}", e))?;
                    return Ok(UserDataStreamEvent::TradeLite(stream));
                }
                "ACCOUNT_UPDATE" => {
                    let stream: AccountUpdateStream = serde_json::from_str(response)
                        .map_err(|e| format!("ACCOUNT_UPDATE解析失败: {}", e))?;
                    return Ok(UserDataStreamEvent::AccountUpdate(stream));
                }
                _ => {}
            }
        }
    }
    
    Ok(UserDataStreamEvent::Unknown(json))
}
