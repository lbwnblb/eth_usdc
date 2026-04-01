use futures_util::{SinkExt, StreamExt};
use log::{error, info, warn};
use std::str::FromStr;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::spawn;
use tokio::sync::{Mutex, watch};
use tokio::time::{interval, Duration};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};
use tokio_tungstenite::tungstenite::handshake::client::Response;
use lazy_static::lazy_static;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use crate::logger::init_logger;
use crate::utils::get_env;
use crate::ws_api::{login, parse_login_response, is_login_success, check_response_id, create_user_data_stream_request, parse_user_data_stream_response, create_ping_user_data_stream_request, parse_ping_response, create_market_subscribe_request, create_account_balance_request, parse_account_balance_response, parse_book_ticker, create_order_request, parse_user_data_stream, UserDataStreamEvent};
use crate::ed25519::{get_api_key, get_private_key};
use crate::utils::get_server_time;

lazy_static! {
    pub static ref USDC_AVAILABLE_BALANCE: Arc<Mutex<Decimal>> = Arc::new(Mutex::new(dec!(0.0)));
    pub static ref USDT_AVAILABLE_BALANCE: Arc<Mutex<Decimal>> = Arc::new(Mutex::new(dec!(0.0)));
    pub static ref ORDER_PLACED: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
}

mod ed25519;
mod utils;
mod logger;
mod ws_api;

type WsReadHalf = futures_util::stream::SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;
type WsWriteHalf = futures_util::stream::SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>;

async fn try_reconnect(
    ws_url: &str,
    write_arc: &Arc<Mutex<WsWriteHalf>>,
) -> Option<WsReadHalf> {
    match connect_async(ws_url).await {
        Ok((ws_stream, _)) => {
            info!("Reconnected to WebSocket");
            let (write_reconnected, read_reconnected) = ws_stream.split();
            let mut guard = write_arc.lock().await;
            *guard = write_reconnected;
            Some(read_reconnected)
        }
        Err(_) => None,
    }
}

async fn connect_user_data_stream(listen_key: &str) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        let ws_url = utils::get_ws_baseurl(listen_key);
        info!("Connecting to user data stream: {}", ws_url);

        match connect_async(ws_url).await {
            Ok((ws_stream, _)) => {
                info!("User data stream connected successfully");
                let (mut write, mut read) = ws_stream.split();
                
                let stream_name = format!("{}@ACCOUNT_UPDATE", listen_key);
                let subscribe_request = serde_json::json!({
                    "method": "SUBSCRIBE",
                    "params": [stream_name],
                    "id": 1
                });
                let subscribe_text = serde_json::to_string(&subscribe_request).expect("JSON serialization failed");
                info!("Sending subscribe request: {}", subscribe_text);
                if let Err(e) = write.send(Message::Text(subscribe_text)).await {
                    error!("Failed to send subscribe request: {}", e);
                }

                while let Some(msg) = read.next().await {
                    match msg {
                        Ok(Message::Text(text)) => {
                            match parse_user_data_stream(&text) {
                                Ok(event) => {
                                    match event {
                                        UserDataStreamEvent::OrderTradeUpdate(update) => {
                                            info!("订单交易更新 - 订单ID: {}, 状态: {}, 交易对: {}, 方向: {}, 价格: {}, 已成交: {}, 手续费: {} {}",
                                                update.data.order.order_id,
                                                update.data.order.order_status,
                                                update.data.order.symbol,
                                                update.data.order.side,
                                                update.data.order.price,
                                                update.data.order.filled_accumulated_qty,
                                                update.data.order.commission,
                                                update.data.order.commission_asset.as_deref().unwrap_or("")
                                            );
                                        }
                                        UserDataStreamEvent::TradeLite(trade) => {
                                            info!("交易信息 - 订单ID: {}, 交易对: {}, 方向: {}, 价格: {}, 成交量: {}, 交易ID: {}",
                                                trade.data.order_id,
                                                trade.data.symbol,
                                                trade.data.side,
                                                trade.data.price,
                                                trade.data.last_filled_qty,
                                                trade.data.trade_id
                                            );
                                        }
                                        UserDataStreamEvent::AccountUpdate(update) => {
                                            info!("账户更新 - 原因: {}", update.data.account.reason);
                                            for balance in &update.data.account.balances {
                                                info!("  资产: {}, 钱包余额: {}, 可用余额: {}",
                                                    balance.asset,
                                                    balance.wallet_balance,
                                                    balance.cross_wallet_balance
                                                );
                                            }
                                            for position in &update.data.account.positions {
                                                info!("  持仓: {}, 数量: {}, 入场价: {}, 未实现盈亏: {}",
                                                    position.symbol,
                                                    position.position_amount,
                                                    position.entry_price,
                                                    position.unrealized_pnl
                                                );
                                            }
                                        }
                                        UserDataStreamEvent::Unknown(json) => {
                                            info!("收到未知类型的用户数据流消息: {}", json);
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("解析用户数据流消息失败: {}", e);
                                }
                            }
                        }
                        Ok(Message::Close(_)) => {
                            warn!("User data stream closed by server");
                            break;
                        }
                        Err(e) => {
                            error!("User data stream error: {}", e);
                            break;
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => {
                error!("Failed to connect to user data stream: {}", e);
            }
        }

        info!("Reconnecting in 5 seconds...");
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

async fn connect_market_stream() -> Result<(), Box<dyn std::error::Error>> {
    loop {
        let ws_url = utils::get_ws_market_baseurl();
        info!("Connecting to market stream: {}", ws_url);

        match connect_async(ws_url).await {
            Ok((ws_stream, _)) => {
                info!("Market stream connected successfully");
                let (mut write, mut read) = ws_stream.split();

                let symbol_lower = utils::get_symbol().to_lowercase();
                let agg_trade_stream = format!("{}@aggTrade", symbol_lower);
                let subscribe_text = create_market_subscribe_request(1, vec![agg_trade_stream.as_str()]);
                info!("Sending subscribe request: {}", subscribe_text);
                write.send(Message::Text(subscribe_text)).await?;

                while let Some(msg) = read.next().await {
                    match msg {
                        Ok(Message::Text(text)) => {
                            // info!("Market stream received: {}", text);
                        }
                        Ok(Message::Close(_)) => {
                            warn!("Market stream closed by server");
                            break;
                        }
                        Err(e) => {
                            error!("Market stream error: {}", e);
                            break;
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => {
                error!("Failed to connect to market stream: {}", e);
            }
        }

        info!("Reconnecting market stream in 5 seconds...");
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

async fn connect_public_stream(write_arc: Arc<Mutex<WsWriteHalf>>, api_key: String, private_key: String) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        let ws_url = utils::get_ws_public_baseurl();
        info!("Connecting to public stream: {}", ws_url);

        match connect_async(ws_url).await {
            Ok((ws_stream, _)) => {

                let symbol = utils::get_symbol();
                let symbol_lower = symbol.to_lowercase();
                let book_ticker_stream = format!("{}@bookTicker", symbol_lower);

                info!("Public stream connected successfully");
                let (mut write, mut read) = ws_stream.split();

                let subscribe_text = create_market_subscribe_request(1, vec![book_ticker_stream.as_str()]);
                info!("Sending subscribe request for {}: {}", book_ticker_stream, subscribe_text);
                write.send(Message::Text(subscribe_text)).await?;

                while let Some(msg) = read.next().await {
                    match msg {
                        Ok(Message::Text(text)) => {
                            // info!("Public stream received: {}", text);
                            match parse_book_ticker(&text) {
                                Ok(book_ticker) => {
                                    // info!("Parsed BookTicker: stream={}, symbol={}, bid_price={}, bid_qty={}, ask_price={}, ask_qty={}",
                                    //     book_ticker.stream,
                                    //     book_ticker.data.symbol,
                                    //     book_ticker.data.best_bid_price,
                                    //     book_ticker.data.best_bid_qty,
                                    //     book_ticker.data.best_ask_price,
                                    //     book_ticker.data.best_ask_qty
                                    // );
                                    
                                    let mut order_placed = ORDER_PLACED.lock().await;
                                    if !*order_placed {
                                        info!("Preparing to place order...");
                                        
                                        let bid_price = match book_ticker.data.best_bid_price.parse::<f64>() {
                                            Ok(p) => p,
                                            Err(e) => {
                                                error!("Failed to parse bid price: {}", e);
                                                continue;
                                            }
                                        };
                                        
                                        let timestamp = match get_server_time().await {
                                            Ok(t) => t,
                                            Err(e) => {
                                                error!("Failed to get server time: {}", e);
                                                continue;
                                            }
                                        };
                                        
                                        let order_request = create_order_request(
                                            "place_order",
                                            symbol,
                                            "BUY",
                                            "LIMIT",
                                            0.01,
                                            Some(bid_price),
                                            Some("GTX"),
                                            None,
                                            timestamp
                                        );
                                        
                                        info!("Sending order request: {}", order_request);
                                        let mut write_guard = write_arc.lock().await;
                                        if let Err(e) = write_guard.send(Message::Text(order_request)).await {
                                            error!("Failed to send order request: {}", e);
                                        } else {
                                            *order_placed = true;
                                            info!("Order request sent successfully!");
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to parse BookTicker: {}", e);
                                }
                            }
                        }
                        Ok(Message::Close(_)) => {
                            warn!("Public stream closed by server");
                            break;
                        }
                        Err(e) => {
                            error!("Public stream error: {}", e);
                            break;
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => {
                error!("Failed to connect to public stream: {}", e);
            }
        }

        info!("Reconnecting public stream in 5 seconds...");
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

async fn connect_websocket() -> Result<(String, Arc<Mutex<WsWriteHalf>>, String, String), Box<dyn std::error::Error>> {
    let ws_url = utils::get_ws_api_baseurl();
    info!("Connecting to WebSocket: {}", ws_url);

    let (listen_key_tx, mut listen_key_rx) = watch::channel(String::new());
    let listen_key = Arc::new(Mutex::new(String::new()));
    let listen_key_clone = listen_key.clone();

    match connect_async(ws_url).await {
        Ok((ws_stream, _)) => {
            info!("WebSocket connected successfully");
            let api_key = get_api_key().await;
            let private_key = get_private_key().await;
            let api_key_for_login = api_key.clone();
            let private_key_for_login = private_key.clone();
            let (write, mut read) = ws_stream.split();

            let write_arc = Arc::new(Mutex::new(write));
            let write_for_ping = write_arc.clone();
            let write_for_read = write_arc.clone();
            let write_arc_clone = write_arc.clone();

            let timestamp = get_server_time().await?;
            let auth_id = "login";
            let auth_request = login(&auth_id, &api_key_for_login, &private_key_for_login, timestamp);
            
            info!("Sending auth request: {}", auth_request);
            {
                let mut write_guard = write_arc.lock().await;
                if let Err(e) = write_guard.send(Message::Text(auth_request)).await {
                    error!("Failed to send auth request: {}", e);
                    return Err(e.into());
                }
            }

            let user_data_stream_start_id = "user_data_stream_start";
            let ping_id = "user_data_stream_ping";
            let account_balance_id = "account_balance";
            let api_key_clone_for_ping = api_key.clone();
            let api_key_clone_for_return = api_key.clone();
            let api_key_clone_for_read = api_key.clone();
            let private_key_clone_for_return = private_key.clone();
            let private_key_clone_for_read = private_key.clone();
            
            let _ = spawn(async move {
                let mut ping_interval = interval(Duration::from_secs(59 * 60));
                ping_interval.tick().await;
                
                loop {
                    ping_interval.tick().await;
                    
                    let key = listen_key_clone.lock().await;
                    if !key.is_empty() {
                        let ping_request = create_ping_user_data_stream_request(ping_id, &api_key_clone_for_ping);
                        info!("Sending ping request to renew listenKey: {}", ping_request);
                        let mut write_guard = write_for_ping.lock().await;
                        if let Err(e) = write_guard.send(Message::Text(ping_request)).await {
                            error!("Failed to send ping request: {}", e);
                        }
                    }
                }
            });

            let _ = spawn(async move {
                while let Some(msg) = read.next().await {
                    match msg {
                        Ok(Message::Text(text)) => {
                            info!("Received: {}", text);

                            if check_response_id(&text, auth_id) {
                                if is_login_success(&text) {
                                    match parse_login_response(&text) {
                                        Ok(_) => {
                                            info!("登录成功");
                                            let user_data_stream_start_request = create_user_data_stream_request(user_data_stream_start_id, &api_key_clone_for_read);
                                            info!("Sending user data stream start request: {}", user_data_stream_start_request);
                                            let mut write_guard = write_for_read.lock().await;
                                            if let Err(e) = write_guard.send(Message::Text(user_data_stream_start_request)).await {
                                                error!("Failed to send user data stream start request: {}", e);
                                                break;
                                            }
                                            
                                            let balance_timestamp = get_server_time().await.unwrap_or(timestamp);
                                            let account_balance_request = create_account_balance_request(account_balance_id, &api_key_clone_for_read, &private_key_clone_for_read, balance_timestamp);
                                            info!("Sending account balance request: {}", account_balance_request);
                                            if let Err(e) = write_guard.send(Message::Text(account_balance_request)).await {
                                                error!("Failed to send account balance request: {}", e);
                                            }
                                        }
                                        Err(e) => {
                                            error!("Failed to parse response: {}", e);
                                        }
                                    }
                                } else {
                                    warn!("Login failed or invalid response");
                                }
                            }

                            if check_response_id(&text, user_data_stream_start_id) {
                                match parse_user_data_stream_response(&text) {
                                    Ok(resp) => {
                                        if resp.status == 200 {
                                            if let Some(result) = resp.result {
                                                let key = result.listenKey.clone();
                                                info!("User data stream started successfully, listenKey: {}", key);
                                                let mut listen_key_guard = listen_key.lock().await;
                                                *listen_key_guard = key.clone();
                                                let _ = listen_key_tx.send(key);
                                            }
                                        } else {
                                            warn!("User data stream start failed");
                                        }
                                    }
                                    Err(e) => {
                                        error!("Failed to parse user data stream response: {}", e);
                                    }
                                }
                            }

                            if check_response_id(&text, ping_id) {
                                match parse_ping_response(&text) {
                                    Ok(resp) => {
                                        if resp.status == 200 {
                                            if let Some(result) = resp.result {
                                                info!("listenKey renewed successfully, listenKey: {}", result.listenKey);
                                                let mut listen_key_guard = listen_key.lock().await;
                                                *listen_key_guard = result.listenKey;
                                            }
                                        } else {
                                            warn!("listenKey renew failed");
                                        }
                                    }
                                    Err(e) => {
                                        error!("Failed to parse ping response: {}", e);
                                    }
                                }
                            }

                            if check_response_id(&text, account_balance_id) {
                                match parse_account_balance_response(&text) {
                                    Ok(resp) => {
                                        if resp.status == 200 {
                                            if let Some(result) = resp.result {
                                                info!("Account balance retrieved successfully");
                                                for balance in result {
                                                    info!("Asset: {}, Balance: {}, Available: {}", balance.asset, balance.balance, balance.available_balance);
                                                    if balance.asset.to_lowercase() == "usdc" {
                                                        match Decimal::from_str(&balance.available_balance) {
                                                            Ok(decimal_balance) => {
                                                                let mut usdc_balance = USDC_AVAILABLE_BALANCE.lock().await;
                                                                *usdc_balance = decimal_balance;
                                                                info!("Updated USDC available balance: {}", decimal_balance);
                                                            }
                                                            Err(e) => {
                                                                error!("Failed to parse USDC balance to Decimal: {}", e);
                                                            }
                                                        }
                                                    }
                                                    if balance.asset.to_lowercase() == "usdt" {
                                                        match Decimal::from_str(&balance.available_balance) {
                                                            Ok(decimal_balance) => {
                                                                let mut usdt_balance = USDT_AVAILABLE_BALANCE.lock().await;
                                                                *usdt_balance = decimal_balance;
                                                                info!("Updated USDT available balance: {}", decimal_balance);
                                                            }
                                                            Err(e) => {
                                                                error!("Failed to parse USDT balance to Decimal: {}", e);
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        } else {
                                            warn!("Account balance request failed");
                                        }
                                    }
                                    Err(e) => {
                                        error!("Failed to parse account balance response: {}", e);
                                    }
                                }
                            }
                        }
                        Ok(Message::Close(_)) => {
                            warn!("WebSocket closed by server");
                            if let Some(new_read) = try_reconnect(&ws_url, &write_arc).await {
                                read = new_read;
                            }

                        }
                        Err(e) => {
                            error!("WebSocket error: {}", e);
                            warn!("WebSocket closed by server");
                            if let Some(new_read) = try_reconnect(&ws_url, &write_arc).await {
                                read = new_read;
                            }
                        }
                        _ => {}
                    }
                }
            });
            
            info!("Waiting for listenKey...");
            listen_key_rx.changed().await?;
            let key = listen_key_rx.borrow().clone();
            info!("Received listenKey: {}", key);
            
            Ok((key, write_arc_clone, api_key_clone_for_return, private_key_clone_for_return))
        }
        Err(e) => {
            error!("Failed to connect to WebSocket: {}", e);
            return Err(e.into());
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = init_logger("logs");
    let (listen_key, write_arc, api_key, private_key) = connect_websocket().await?;
    info!("Final listenKey received: {}", listen_key);
    info!("WebSocket write_arc received");
    
    let listen_key_clone = listen_key.clone();
    spawn(async move {
        if let Err(e) = connect_user_data_stream(&listen_key_clone).await {
            error!("User data stream connection failed: {}", e);
        }
    });
    
    spawn(async move {
        if let Err(e) = connect_market_stream().await {
            error!("Market stream connection failed: {}", e);
        }
    });
    
    if let Err(e) = connect_public_stream(write_arc, api_key, private_key).await {
        error!("Public stream connection failed: {}", e);
    }
    Ok(())
}
