use futures_util::{SinkExt, StreamExt};
use log::{error, info, warn};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::spawn;
use tokio::sync::{Mutex, watch};
use tokio::time::{interval, Duration};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};
use tokio_tungstenite::tungstenite::handshake::client::Response;
use crate::logger::init_logger;
use crate::utils::get_env;
use crate::ws_api::{login, parse_login_response, is_login_success, check_response_id, create_user_data_stream_request, parse_user_data_stream_response, create_ping_user_data_stream_request, parse_ping_response, create_market_subscribe_request, create_account_balance_request, parse_account_balance_response};
use crate::ed25519::{get_api_key, get_private_key};
use crate::utils::get_server_time;

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
                let (_, mut read) = ws_stream.split();

                while let Some(msg) = read.next().await {
                    match msg {
                        Ok(Message::Text(text)) => {
                            info!("User data stream received: {}", text);
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

                let subscribe_text = create_market_subscribe_request(1, vec!["btcusdt@aggTrade", "btcusdt@depth"]);
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

async fn connect_websocket() -> Result<(String, Arc<Mutex<WsWriteHalf>>), Box<dyn std::error::Error>> {
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
            let (write, mut read) = ws_stream.split();

            let write_arc = Arc::new(Mutex::new(write));
            let write_for_ping = write_arc.clone();
            let write_for_read = write_arc.clone();
            let write_arc_clone = write_arc.clone();

            let timestamp = get_server_time().await?;
            let auth_id = "login";
            let auth_request = login(&auth_id, api_key, private_key, timestamp);
            
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
            let api_key_clone = api_key.clone();
            let private_key_clone = private_key.clone();
            
            let _ = spawn(async move {
                let mut ping_interval = interval(Duration::from_secs(59 * 60));
                ping_interval.tick().await;
                
                loop {
                    ping_interval.tick().await;
                    
                    let key = listen_key_clone.lock().await;
                    if !key.is_empty() {
                        let ping_request = create_ping_user_data_stream_request(ping_id, &api_key_clone);
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
                                            let user_data_stream_start_request = create_user_data_stream_request(user_data_stream_start_id, api_key);
                                            info!("Sending user data stream start request: {}", user_data_stream_start_request);
                                            let mut write_guard = write_for_read.lock().await;
                                            if let Err(e) = write_guard.send(Message::Text(user_data_stream_start_request)).await {
                                                error!("Failed to send user data stream start request: {}", e);
                                                break;
                                            }
                                            
                                            let balance_timestamp = get_server_time().await.unwrap_or(timestamp);
                                            let account_balance_request = create_account_balance_request(account_balance_id, api_key, private_key, balance_timestamp);
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
            
            Ok((key, write_arc_clone))
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
    let (listen_key, write_arc) = connect_websocket().await?;
    info!("Final listenKey received: {}", listen_key);
    info!("WebSocket write_arc received");
    
    let listen_key_clone = listen_key.clone();
    tokio::spawn(async move {
        if let Err(e) = connect_user_data_stream(&listen_key_clone).await {
            error!("User data stream connection failed: {}", e);
        }
    });
    
    if let Err(e) = connect_market_stream().await {
        error!("Market stream connection failed: {}", e);
    }
    Ok(())
}
