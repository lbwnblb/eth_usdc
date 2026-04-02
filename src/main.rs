use futures_util::{SinkExt, StreamExt};
use std::str::FromStr;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::spawn;
use tokio::sync::{Mutex, watch};
use tokio::time::{interval, Duration};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};
use lazy_static::lazy_static;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use crate::logger::init_logger;
use crate::utils::{calc_quantity, get_exchange_info, get_server_time};
use crate::ws_api::{login, parse_login_response, is_login_success, check_response_id, create_user_data_stream_request, parse_user_data_stream_response, create_ping_user_data_stream_request, parse_ping_response, create_market_subscribe_request, create_account_balance_request, parse_account_balance_response, parse_book_ticker, create_order_request, create_cancel_order_request, parse_user_data_stream, UserDataStreamEvent, parse_generic_response, BookTickerStream, OrderTradeUpdateStream, TradeLiteStream, parse_agg_trade};
use crate::ed25519::{get_api_key, get_private_key};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrderSide {
    Buy,
    Sell,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrderStatus {
    New,
    PartiallyFilled,
    Filled,
    Canceled,
    Rejected,
    Expired,
    Selling,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuoteAsset {
    USDT,
    USDC,
}

#[derive(Debug, Clone)]
pub struct Order {
    pub id: String,
    pub order_id: String,
    pub client_order_id: String,
    pub symbol: String,
    pub side: OrderSide,
    pub status: OrderStatus,
    pub price: Decimal,
    pub quantity: Decimal,
    pub filled_quantity: Decimal,
    pub quote_asset: QuoteAsset,
    pub create_time: u64,
    pub update_time: u64,
    pub deducted_amount: Decimal,
    pub related_buy_order_client_id: Option<String>,
    pub timeout_processed: bool,
}

pub struct GlobalOrderManager {
    pub orders: Vec<Order>,
    pub total_count: usize,
    pub last_buy_order_time: u64,
    pub last_sell_order_time: u64,
}

lazy_static! {
    pub static ref USDC_AVAILABLE_BALANCE: Arc<Mutex<Decimal>> = Arc::new(Mutex::new(dec!(0.0)));
    pub static ref USDT_AVAILABLE_BALANCE: Arc<Mutex<Decimal>> = Arc::new(Mutex::new(dec!(0.0)));
    pub static ref USDT_TO_USE: Arc<Mutex<Decimal>> = Arc::new(Mutex::new(dec!(500)));
    pub static ref USDC_TO_USE: Arc<Mutex<Decimal>> = Arc::new(Mutex::new(dec!(500)));
    pub static ref ORDER_MANAGER: Arc<Mutex<GlobalOrderManager>> = Arc::new(Mutex::new(GlobalOrderManager { orders: Vec::new(), total_count: 0, last_buy_order_time: 0, last_sell_order_time: 0 }));
    pub static ref LAST_PRICE: Arc<Mutex<Option<Decimal>>> = Arc::new(Mutex::new(None));
    pub static ref PRICE_GAPS: Arc<Mutex<Vec<Decimal>>> = Arc::new(Mutex::new(Vec::new()));
}

fn calculate_median(gaps: &mut Vec<Decimal>) -> Option<Decimal> {
    if gaps.is_empty() {
        return None;
    }
    
    gaps.sort_by(|a, b| b.cmp(a));
    let len = gaps.len();
    
    if len % 2 == 1 {
        Some(gaps[len / 2])
    } else {
        let mid_left = gaps[(len - 1) / 2];
        let mid_right = gaps[len / 2];
        Some((mid_left + mid_right) / dec!(2.0))
    }
}

mod ed25519;
mod utils;
mod logger;
mod ws_api;

type WsReadHalf = futures_util::stream::SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;
type WsWriteHalf = futures_util::stream::SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>;

async fn handle_order_trade_update(update: OrderTradeUpdateStream) {
    info!("订单交易更新 - 订单ID: {}, 用户自定义订单号: {}, 状态: {}, 交易对: {}, 方向: {}, 价格: {}, 已成交: {}, 手续费: {} {}",
        update.data.order.order_id,
        update.data.order.client_order_id,
        update.data.order.order_status,
        update.data.order.symbol,
        update.data.order.side,
        update.data.order.price,
        update.data.order.filled_accumulated_qty,
        update.data.order.commission,
        update.data.order.commission_asset.as_deref().unwrap_or("")
    );
    
    let client_order_id = &update.data.order.client_order_id;
    let mut order_manager = ORDER_MANAGER.lock().await;
    
    let is_sell_order = update.data.order.side.to_uppercase() == "SELL";
    let is_filled = update.data.order.order_status.as_str() == "FILLED";
    let is_canceled = update.data.order.order_status.as_str() == "CANCELED" 
        || update.data.order.order_status.as_str() == "EXPIRED"
        || update.data.order.order_status.as_str() == "REJECTED";
    
    if is_sell_order && is_filled {
        let mut related_buy_client_id = None;
        let mut timeout_processed = false;
        if let Some(sell_order) = order_manager.orders.iter().find(|o| o.client_order_id == *client_order_id) {
            related_buy_client_id = sell_order.related_buy_order_client_id.clone();
            timeout_processed = sell_order.timeout_processed;
        }
        
        if let Some(buy_client_id) = related_buy_client_id {
            let initial_len = order_manager.orders.len();
            order_manager.orders.retain(|o| o.client_order_id != *client_order_id && o.client_order_id != buy_client_id);
            let removed = initial_len - order_manager.orders.len();
            if removed > 0 {
                if !timeout_processed {
                    order_manager.total_count -= 1;
                    info!("卖出订单全部成交，已剔除买单(自定义ID: {})和卖单(自定义ID: {})，总仓位减1，当前总仓位: {}", buy_client_id, client_order_id, order_manager.total_count);
                } else {
                    info!("卖出订单全部成交，但已超时处理过，总仓位不再减1，已剔除买单(自定义ID: {})和卖单(自定义ID: {})，当前总仓位: {}", buy_client_id, client_order_id, order_manager.total_count);
                }
            }
        }
    } else if is_canceled && !is_sell_order {
        let mut deducted_amount = None;
        let mut quote_asset = None;
        
        if let Some(order) = order_manager.orders.iter().find(|o| o.client_order_id == *client_order_id) {
            deducted_amount = Some(order.deducted_amount);
            quote_asset = Some(order.quote_asset.clone());
        }
        
        if let (Some(amount), Some(asset)) = (deducted_amount, quote_asset) {
            match asset {
                QuoteAsset::USDT => {
                    let mut usdt_balance = USDT_AVAILABLE_BALANCE.lock().await;
                    *usdt_balance += amount;
                    info!("订单取消/过期，已恢复 {} USDT 到可用余额，剩余: {}", amount, usdt_balance);
                },
                QuoteAsset::USDC => {
                    let mut usdc_balance = USDC_AVAILABLE_BALANCE.lock().await;
                    *usdc_balance += amount;
                    info!("订单取消/过期，已恢复 {} USDC 到可用余额，剩余: {}", amount, usdc_balance);
                },
            }
        }
        
        let initial_len = order_manager.orders.len();
        order_manager.orders.retain(|o| o.client_order_id != *client_order_id);
        let removed = initial_len - order_manager.orders.len();
        if removed > 0 {
            order_manager.total_count -= removed;
            info!("已从 ORDER_MANAGER 中删除取消/过期的买单，自定义订单号: {}, 剩余订单总数: {}", client_order_id, order_manager.total_count);
        }
    } else {
        if let Some(order) = order_manager.orders.iter_mut().find(|o| o.client_order_id == *client_order_id) {
            order.order_id = update.data.order.order_id.to_string();
            
            match update.data.order.order_status.as_str() {
                "NEW" => order.status = OrderStatus::New,
                "PARTIALLY_FILLED" => order.status = OrderStatus::PartiallyFilled,
                "FILLED" => order.status = OrderStatus::Filled,
                "CANCELED" => order.status = OrderStatus::Canceled,
                "REJECTED" => order.status = OrderStatus::Rejected,
                "EXPIRED" => order.status = OrderStatus::Expired,
                _ => {}
            }
            
            if let Ok(filled_qty) = Decimal::from_str(&update.data.order.filled_accumulated_qty) {
                order.filled_quantity = filled_qty;
            }
            
            order.update_time = update.data.order.transaction_time;
            
            info!("已更新全局订单 - 自定义订单号: {}, 状态: {:?}, 已成交: {}", 
                client_order_id, order.status, order.filled_quantity);
        }
    }
}

async fn handle_trade_lite(trade: TradeLiteStream) {
    info!("交易信息 - 订单ID: {}, 交易对: {}, 方向: {}, 价格: {}, 成交量: {}, 交易ID: {}, 自定义订单号: {}",
        trade.data.order_id,
        trade.data.symbol,
        trade.data.side,
        trade.data.price,
        trade.data.last_filled_qty,
        trade.data.trade_id,
        trade.data.client_order_id
    );
    
    let client_order_id = &trade.data.client_order_id;
    let is_sell_order = trade.data.side.to_uppercase() == "SELL";
    let mut order_manager = ORDER_MANAGER.lock().await;
    
    if let Some(order) = order_manager.orders.iter_mut().find(|o| o.client_order_id == *client_order_id) {
        order.order_id = trade.data.order_id.to_string();
        order.update_time = trade.data.transaction_time;
        
        info!("已更新全局订单(TradeLite) - 自定义订单号: {}", client_order_id);
        
        if is_sell_order && order.filled_quantity >= order.quantity {
            let related_buy_client_id = order.related_buy_order_client_id.clone();
            let timeout_processed = order.timeout_processed;
            
            if let Some(buy_client_id) = related_buy_client_id {
                let initial_len = order_manager.orders.len();
                order_manager.orders.retain(|o| o.client_order_id != *client_order_id && o.client_order_id != buy_client_id);
                let removed = initial_len - order_manager.orders.len();
                if removed > 0 {
                    if !timeout_processed {
                        order_manager.total_count -= 1;
                        info!("卖出订单全部成交(TradeLite)，已剔除买单(自定义ID: {})和卖单(自定义ID: {})，总仓位减1，当前总仓位: {}", buy_client_id, client_order_id, order_manager.total_count);
                    } else {
                        info!("卖出订单全部成交(TradeLite)，但已超时处理过，总仓位不再减1，已剔除买单(自定义ID: {})和卖单(自定义ID: {})，当前总仓位: {}", buy_client_id, client_order_id, order_manager.total_count);
                    }
                }
            }
        }
    }
}

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
                            info!("connect_user_data_stream message: {}", text);
                            match parse_user_data_stream(&text) {
                                Ok(event) => {
                                    match event {
                                        UserDataStreamEvent::OrderTradeUpdate(update) => {
                                            handle_order_trade_update(update).await;
                                        }
                                        UserDataStreamEvent::TradeLite(trade) => {
                                            handle_trade_lite(trade).await;
                                        }
                                        UserDataStreamEvent::AccountUpdate(update) => {
                                            info!("账户更新 - 原因: {}", update.data.account.reason);
                                            for balance in &update.data.account.balances {
                                                info!("  资产: {}, 钱包余额: {}, 可用余额: {}",
                                                    balance.asset,
                                                    balance.wallet_balance,
                                                    balance.cross_wallet_balance
                                                );
                                                if balance.asset.to_lowercase() == "usdc" {
                                                    match Decimal::from_str(&balance.cross_wallet_balance) {
                                                        Ok(decimal_balance) => {
                                                            let mut usdc_balance = USDC_AVAILABLE_BALANCE.lock().await;
                                                            *usdc_balance = decimal_balance;
                                                            info!("更新 USDC 可用余额: {}", decimal_balance);
                                                        }
                                                        Err(e) => {
                                                            error!("解析 USDC 余额失败: {}", e);
                                                        }
                                                    }
                                                }
                                                if balance.asset.to_lowercase() == "usdt" {
                                                    match Decimal::from_str(&balance.cross_wallet_balance) {
                                                        Ok(decimal_balance) => {
                                                            let mut usdt_balance = USDT_AVAILABLE_BALANCE.lock().await;
                                                            *usdt_balance = decimal_balance;
                                                            info!("更新 USDT 可用余额: {}", decimal_balance);
                                                        }
                                                        Err(e) => {
                                                            error!("解析 USDT 余额失败: {}", e);
                                                        }
                                                    }
                                                }
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
                            match parse_agg_trade(&text) {
                                Ok(agg_trade) => {
                                    info!("聚合交易 - 交易对: {}, 价格: {}, 数量: {}, 普通数量: {}, 买方做市: {}, 交易ID: {}",
                                        agg_trade.data.symbol,
                                        agg_trade.data.price,
                                        agg_trade.data.quantity,
                                        agg_trade.data.normal_quantity,
                                        agg_trade.data.is_buyer_maker,
                                        agg_trade.data.aggregate_trade_id
                                    );
                                    
                                    if let Ok(current_price) = Decimal::from_str(&agg_trade.data.price) {
                                        let mut last_price_guard = LAST_PRICE.lock().await;
                                        
                                        if let Some(last_price) = *last_price_guard {
                                            let gap = current_price - last_price;
                                            
                                            if gap != dec!(0.0) {
                                                let mut price_gaps_guard = PRICE_GAPS.lock().await;
                                                price_gaps_guard.push(gap);
                                                
                                                if price_gaps_guard.len() > 1000 {
                                                    price_gaps_guard.remove(0);
                                                }
                                                
                                                info!("价格间距已记录: {}, 当前间距数组长度: {}", gap, price_gaps_guard.len());
                                            }
                                        }
                                        
                                        *last_price_guard = Some(current_price);
                                    }
                                }
                                Err(e) => {
                                    error!("解析聚合交易失败: {}", e);
                                }
                            }
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

async fn connect_public_stream(write_arc: Arc<Mutex<WsWriteHalf>>, _api_key: String, _private_key: String) -> Result<(), Box<dyn std::error::Error>> {
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
                            info!("Public stream received: {}", text);
                            match parse_book_ticker(&text) {
                                Ok(book_ticker) => {
                                    order_buy(&write_arc, symbol, &book_ticker).await;
                                    order_sell(&write_arc, symbol, &book_ticker).await;
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

async fn order_buy(write_arc: &Arc<Mutex<WsWriteHalf>>, symbol: &str, book_ticker: &BookTickerStream) -> bool {
    info!("Preparing to place order...");

    let bid_price = match Decimal::from_str(&book_ticker.data.best_bid_price) {
        Ok(p) => p,
        Err(e) => {
            error!("Failed to parse bid price: {}", e);
            return true;
        }
    };

    if symbol == "ETHUSDC" && bid_price > dec!(3000) {
        info!("交易对是ETHUSDC且最佳买入价格({})超过3000，跳过此次买入", bid_price);
        return true;
    }

    let timestamp = book_ticker.data.event_time as i64;

    let order_manager = ORDER_MANAGER.lock().await;
    let ten_seconds_ms = 10 * 1000;
    if order_manager.total_count >= 1 {
        info!("跳过此次买入请求，当前总仓位: {}", order_manager.total_count);
        return true;
    }
    if order_manager.last_buy_order_time != 0 && (timestamp as u64) - order_manager.last_buy_order_time < ten_seconds_ms {
        info!("距离上次买入订单不足10秒，跳过此次请求，距离下次可请求还需 {} 毫秒", ten_seconds_ms - ((timestamp as u64) - order_manager.last_buy_order_time));
        return true;
    }
    drop(order_manager);

    let quote_asset = if symbol.ends_with("USDC") {
        QuoteAsset::USDC
    } else {
        QuoteAsset::USDT
    };

    let (balance_to_use, balance_name, available_balance) = match quote_asset {
        QuoteAsset::USDT => {
            let mut usdt_to_use_global = USDT_TO_USE.lock().await;
            let usdt_balance = USDT_AVAILABLE_BALANCE.lock().await;
            let available = *usdt_balance;
            drop(usdt_balance);
            let usdt_to_use = if *usdt_to_use_global == dec!(0.0) {
                let usdt_balance = USDT_AVAILABLE_BALANCE.lock().await;
                let calculated = *usdt_balance / dec!(10);
                drop(usdt_balance);
                let mut global = usdt_to_use_global;
                *global = calculated;
                calculated
            } else {
                *usdt_to_use_global
            };
            (usdt_to_use, "USDT", available)
        },
        QuoteAsset::USDC => {
            let mut usdc_to_use_global = USDC_TO_USE.lock().await;
            let usdc_balance = USDC_AVAILABLE_BALANCE.lock().await;
            let available = *usdc_balance;
            drop(usdc_balance);
            let usdc_to_use = if *usdc_to_use_global == dec!(0.0) {
                let usdc_balance = USDC_AVAILABLE_BALANCE.lock().await;
                let calculated = *usdc_balance / dec!(10);
                drop(usdc_balance);
                let mut global = usdc_to_use_global;
                *global = calculated;
                calculated
            } else {
                *usdc_to_use_global
            };
            (usdc_to_use, "USDC", available)
        },
    };

    if balance_to_use <= dec!(0.0) {
        error!("Insufficient {} balance to place order", balance_name);
        return true;
    }

    if available_balance < balance_to_use {
        info!("可用余额 {} {} 小于买入金额 {} {}，跳过此次买入", available_balance, balance_name, balance_to_use, balance_name);
        return true;
    }

    let exchange_info = match get_exchange_info().await {
        Ok(info) => info,
        Err(e) => {
            error!("Failed to get exchange info: {}", e);
            return true;
        }
    };

    let symbol_info = match exchange_info.symbols.into_iter().find(|s| s.symbol == symbol) {
        Some(info) => info,
        None => {
            error!("Symbol {} not found in exchange info", symbol);
            return true;
        }
    };

    let quantity = match calc_quantity(
        &balance_to_use.to_string(),
        symbol,
        &bid_price.to_string(),
        &symbol_info
    ) {
        Ok(qty) => qty,
        Err(e) => {
            error!("Failed to calculate quantity: {}", e);
            return true;
        }
    };

    let quantity_f64 = match quantity.to_string().parse::<f64>() {
        Ok(q) => q,
        Err(e) => {
            error!("Failed to parse quantity to f64: {}", e);
            return true;
        }
    };

    let bid_price_f64 = match bid_price.to_string().parse::<f64>() {
        Ok(p) => p,
        Err(e) => {
            error!("Failed to parse bid price to f64: {}", e);
            return true;
        }
    };

    info!("Calculated order quantity: {} (using {:.2} {}, price: {})", quantity_f64, balance_to_use, balance_name, bid_price);

    let new_client_order_id = format!("{}_{}_{}", symbol, "BUY", timestamp);
    info!("Generated newClientOrderId: {}", new_client_order_id);

    let request_id = format!("place_order_buy_{}", timestamp);
    let order_request = create_order_request(
        &request_id,
        symbol,
        "BUY",
        "LIMIT",
        quantity_f64,
        Some(bid_price_f64),
        Some("GTX"),
        None,
        Some(&new_client_order_id),
        timestamp
    );

    let order = Order {
        id: request_id,
        order_id: String::new(),
        client_order_id: new_client_order_id.clone(),
        symbol: symbol.to_string(),
        side: OrderSide::Buy,
        status: OrderStatus::New,
        price: bid_price,
        quantity: quantity.clone(),
        filled_quantity: dec!(0.0),
        quote_asset: quote_asset.clone(),
        create_time: timestamp as u64,
        update_time: timestamp as u64,
        deducted_amount: balance_to_use,
        related_buy_order_client_id: None,
        timeout_processed: false,
    };
    let mut order_manager = ORDER_MANAGER.lock().await;
    order_manager.orders.push(order);
    order_manager.total_count += 1;
    info!("Order saved to global array, total orders: {}", order_manager.total_count);
    drop(order_manager);

    info!("Sending order request: {}", order_request);
    let send_result = {
        let mut write_guard = write_arc.lock().await;
        write_guard.send(Message::Text(order_request)).await
    };

    let mut send_success = false;
    if let Err(e) = send_result {
        error!("Failed to send order request: {}", e);
    } else {
        info!("Order request sent successfully!");
        send_success = true;
    }

    if send_success {
        let mut order_manager = ORDER_MANAGER.lock().await;
        order_manager.last_buy_order_time = timestamp as u64;
        info!("已更新 last_buy_order_time 为 {}", timestamp);
        drop(order_manager);

        match quote_asset {
            QuoteAsset::USDT => {
                let mut usdt_balance = USDT_AVAILABLE_BALANCE.lock().await;
                *usdt_balance -= balance_to_use;
                info!("已扣除 {} USDT，剩余可用余额: {}", balance_to_use, usdt_balance);
            },
            QuoteAsset::USDC => {
                let mut usdc_balance = USDC_AVAILABLE_BALANCE.lock().await;
                *usdc_balance -= balance_to_use;
                info!("已扣除 {} USDC，剩余可用余额: {}", balance_to_use, usdc_balance);
            },
        }
    }

    false
}

async fn order_sell(write_arc: &Arc<Mutex<WsWriteHalf>>, symbol: &str, book_ticker: &BookTickerStream) -> bool {
    let timestamp = book_ticker.data.event_time as i64;
    
    let order_manager = ORDER_MANAGER.lock().await;
    let ten_seconds_ms = 10 * 1000;
    if order_manager.last_sell_order_time != 0 && (timestamp as u64) - order_manager.last_sell_order_time < ten_seconds_ms {
        info!("距离上次卖出订单不足10秒，跳过此次请求，距离下次可请求还需 {} 毫秒", ten_seconds_ms - ((timestamp as u64) - order_manager.last_sell_order_time));
        return false;
    }
    drop(order_manager);
    
    let mut order_manager = ORDER_MANAGER.lock().await;
    
    let mut found_order: Option<(Decimal, Decimal, String)> = None;
    
    let mut price_gaps = PRICE_GAPS.lock().await;
    let median_gap = calculate_median(&mut price_gaps);
    drop(price_gaps);
    
    for order in &order_manager.orders {
        if order.status == OrderStatus::Filled && order.side == OrderSide::Buy {
            let ask_price = match Decimal::from_str(&book_ticker.data.best_ask_price) {
                Ok(p) => p,
                Err(e) => {
                    error!("Failed to parse ask price: {}", e);
                    continue;
                }
            };
            
            let target_price = if let Some(gap) = median_gap {
                order.price + gap
            } else {
                ask_price
            };
            
            let sell_price = if ask_price > target_price {
                ask_price
            } else {
                target_price
            };
            
            found_order = Some((sell_price, order.filled_quantity, order.client_order_id.clone()));
            break;
        }
    }
    
    if let Some((ask_price, filled_quantity, client_order_id)) = found_order {
        if let Some(order) = order_manager.orders.iter_mut().find(|o| o.client_order_id == client_order_id) {
            order.status = OrderStatus::Selling;
            info!("已将买入订单标记为卖出中，客户端订单号: {}", client_order_id);
        }
        
        drop(order_manager);
        
        let timestamp = book_ticker.data.event_time as i64;
        
        let new_client_order_id = format!("{}_{}_{}_sell", symbol, "SELL", timestamp);
        info!("Generated newClientOrderId for sell: {}", new_client_order_id);
        
        let quantity_f64 = match filled_quantity.to_string().parse::<f64>() {
            Ok(q) => q,
            Err(e) => {
                error!("Failed to parse filled quantity to f64: {}", e);
                return false;
            }
        };
        
        let ask_price_f64 = match ask_price.to_string().parse::<f64>() {
            Ok(p) => p,
            Err(e) => {
                error!("Failed to parse ask price to f64: {}", e);
                return false;
            }
        };
        
        let request_id = format!("place_order_sell_{}", timestamp);
        let order_request = create_order_request(
            &request_id,
            symbol,
            "SELL",
            "LIMIT",
            quantity_f64,
            Some(ask_price_f64),
            Some("GTX"),
            None,
            Some(&new_client_order_id),
            timestamp
        );
        
        let sell_order = Order {
            id: request_id.clone(),
            order_id: String::new(),
            client_order_id: new_client_order_id.clone(),
            symbol: symbol.to_string(),
            side: OrderSide::Sell,
            status: OrderStatus::New,
            price: ask_price,
            quantity: filled_quantity.clone(),
            filled_quantity: dec!(0.0),
            quote_asset: if symbol.ends_with("USDC") { QuoteAsset::USDC } else { QuoteAsset::USDT },
            create_time: timestamp as u64,
            update_time: timestamp as u64,
            deducted_amount: dec!(0.0),
            related_buy_order_client_id: Some(client_order_id.clone()),
            timeout_processed: false,
        };
        
        info!("Sending sell order request: {}", order_request);
        let send_result = {
            let mut write_guard = write_arc.lock().await;
            write_guard.send(Message::Text(order_request)).await
        };
        
        if let Err(e) = send_result {
            error!("Failed to send sell order request: {}", e);
            let mut order_manager = ORDER_MANAGER.lock().await;
            if let Some(order) = order_manager.orders.iter_mut().find(|o| o.client_order_id == client_order_id) {
                order.status = OrderStatus::Filled;
                info!("卖出订单发送失败，已将买入订单状态恢复为 Filled，客户端订单号: {}", client_order_id);
            }
        } else {
            info!("Sell order request sent successfully!");
            let mut order_manager = ORDER_MANAGER.lock().await;
            order_manager.orders.push(sell_order);
            order_manager.last_sell_order_time = timestamp as u64;
            info!("已添加卖出订单到全局数组");
            info!("已更新 last_sell_order_time 为 {}", timestamp);
            drop(order_manager);
        }
        
        return true;
    }
    
    false
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

                            match parse_generic_response(&text) {
                                Ok(resp) => {
                                    if resp.id.starts_with("place_order_buy_") {
                                        info!("收到订单响应，ID: {}, 状态: {}", resp.id, resp.status);
                                        if resp.status != 200 {
                                            warn!("订单请求失败，ID: {}, 错误: {:?}", resp.id, resp.error);
                                            let mut order_manager = ORDER_MANAGER.lock().await;
                                            
                                            if let Some(order) = order_manager.orders.iter().find(|o| o.id == resp.id) {
                                                let deducted_amount = order.deducted_amount;
                                                let quote_asset = order.quote_asset.clone();
                                                
                                                if order.side == OrderSide::Buy {
                                                    match quote_asset {
                                                        QuoteAsset::USDT => {
                                                            let mut usdt_balance = USDT_AVAILABLE_BALANCE.lock().await;
                                                            *usdt_balance += deducted_amount;
                                                            info!("已恢复 {} USDT 到可用余额，剩余: {}", deducted_amount, usdt_balance);
                                                        },
                                                        QuoteAsset::USDC => {
                                                            let mut usdc_balance = USDC_AVAILABLE_BALANCE.lock().await;
                                                            *usdc_balance += deducted_amount;
                                                            info!("已恢复 {} USDC 到可用余额，剩余: {}", deducted_amount, usdc_balance);
                                                        },
                                                    }
                                                }
                                            }
                                            
                                            let initial_len = order_manager.orders.len();
                                            order_manager.orders.retain(|order| order.id != resp.id);
                                            let removed = initial_len - order_manager.orders.len();
                                            if removed > 0 {
                                                order_manager.total_count -= removed;
                                                info!("已从 ORDER_MANAGER 中删除失败的订单，ID: {}, 剩余订单总数: {}", resp.id, order_manager.total_count);
                                            }
                                        }
                                    } else if resp.id.starts_with("place_order_sell_") {
                                        info!("收到卖单响应，ID: {}, 状态: {}", resp.id, resp.status);
                                        if resp.status != 200 {
                                            warn!("卖单请求失败，ID: {}, 错误: {:?}", resp.id, resp.error);
                                            let mut order_manager = ORDER_MANAGER.lock().await;
                                            
                                            let mut related_buy_client_id: Option<String> = None;
                                            
                                            if let Some(sell_order) = order_manager.orders.iter().find(|o| o.id == resp.id) {
                                                related_buy_client_id = sell_order.related_buy_order_client_id.clone();
                                            }
                                            
                                            if let Some(buy_client_id) = related_buy_client_id {
                                                if let Some(buy_order) = order_manager.orders.iter_mut().find(|o| o.client_order_id == buy_client_id) {
                                                    buy_order.status = OrderStatus::Filled;
                                                    info!("卖单失败，已将关联买单状态恢复为 Filled，客户端订单号: {}", buy_client_id);
                                                }
                                            }
                                            
                                            let initial_len = order_manager.orders.len();
                                            order_manager.orders.retain(|order| order.id != resp.id);
                                            let removed = initial_len - order_manager.orders.len();
                                            if removed > 0 {
                                                info!("已从 ORDER_MANAGER 中删除失败的卖单，ID: {}", resp.id);
                                            }
                                        }
                                    } else if resp.id.starts_with("cancel_order_") {
                                        info!("收到取消订单响应，ID: {}, 状态: {}", resp.id, resp.status);
                                        if resp.status == 200 {
                                            info!("订单取消成功，ID: {}", resp.id);
                                        } else {
                                            warn!("订单取消失败，ID: {}, 错误: {:?}", resp.id, resp.error);
                                        }
                                    }
                                }
                                Err(_) => {}
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
            Err(e.into())
        }
    }
}

async fn check_and_cancel_expired_orders(write_arc: Arc<Mutex<WsWriteHalf>>) {
    let mut interval = interval(Duration::from_secs(3));
    
    loop {
        interval.tick().await;
        
        let current_timestamp = match get_server_time().await {
            Ok(t) => t,
            Err(e) => {
                error!("获取服务器时间失败: {}", e);
                continue;
            }
        };
        
        let orders_to_cancel: Vec<(String, String)> = {
            let order_manager = ORDER_MANAGER.lock().await;
            let ten_seconds_ms = 10 * 1000;
            
            order_manager
                .orders
                .iter()
                .filter(|order| {
                    order.side == OrderSide::Buy 
                        && order.status == OrderStatus::New 
                        && (current_timestamp as u64) - order.create_time >= ten_seconds_ms
                })
                .map(|order| (order.client_order_id.clone(), order.symbol.clone()))
                .collect()
        };
        
        for (client_order_id, symbol) in orders_to_cancel {
            info!("发现超时买单，准备撤销 - 客户端订单号: {}, 交易对: {}", client_order_id, symbol);
            
            let request_id = format!("cancel_{}", current_timestamp);
            let cancel_request = create_cancel_order_request(
                &request_id,
                &client_order_id,
                &symbol,
                current_timestamp
            );
            
            info!("发送撤销订单请求: {}", cancel_request);
            let send_result = {
                let mut write_guard = write_arc.lock().await;
                write_guard.send(Message::Text(cancel_request)).await
            };
            
            if let Err(e) = send_result {
                error!("发送撤销订单请求失败 - 客户端订单号: {}, 错误: {}", client_order_id, e);
            }
        }
    }
}

async fn check_timeout_sell_orders() {
    let mut interval = interval(Duration::from_secs(30));
    
    loop {
        interval.tick().await;
        
        let current_timestamp = match get_server_time().await {
            Ok(t) => t,
            Err(e) => {
                error!("获取服务器时间失败: {}", e);
                continue;
            }
        };
        
        let one_hour_ms = 60 * 60 * 1000;
        
        let mut order_manager = ORDER_MANAGER.lock().await;
        
        let mut count_to_decrement = 0;
        let mut orders_marked = Vec::new();
        
        for order in &mut order_manager.orders {
            if order.side == OrderSide::Sell 
                && !order.timeout_processed 
                && order.status != OrderStatus::Filled
                && order.status != OrderStatus::Canceled
                && order.status != OrderStatus::Expired
                && order.status != OrderStatus::Rejected
                && (current_timestamp as u64) - order.create_time >= one_hour_ms {
                
                order.timeout_processed = true;
                count_to_decrement += 1;
                orders_marked.push(order.client_order_id.clone());
            }
        }
        
        for client_order_id in orders_marked {
            info!("发现超时卖单，总仓位减1 - 客户端订单号: {}", client_order_id);
        }
        
        if count_to_decrement > 0 {
            order_manager.total_count -= count_to_decrement;
            info!("已标记{}个卖单为超时处理，当前总仓位: {}", count_to_decrement, order_manager.total_count);
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = init_logger("logs");
    let (listen_key, write_arc, api_key, private_key) = connect_websocket().await?;
    info!("Final listenKey received: {}", listen_key);
    info!("WebSocket write_arc received");


    spawn(async move {
        if let Err(e) = connect_market_stream().await {
            error!("Market stream connection failed: {}", e);
        }
    });
    let listen_key_clone = listen_key.clone();
    spawn(async move {
        if let Err(e) = connect_user_data_stream(&listen_key_clone).await {
            error!("User data stream connection failed: {}", e);
        }
    });
    
    let write_arc_for_check = write_arc.clone();
    spawn(async move {
        check_and_cancel_expired_orders(write_arc_for_check).await;
    });

    spawn(async move {
        check_timeout_sell_orders().await;
    });

    if let Err(e) = connect_public_stream(write_arc, api_key, private_key).await {
        error!("Public stream connection failed: {}", e);
    }
    Ok(())
}
