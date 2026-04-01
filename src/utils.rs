use std::sync::OnceLock;

static ENV: OnceLock<String> = OnceLock::new();
static SYMBOL: OnceLock<String> = OnceLock::new();

pub const TEST_ENV: &str = "test";
pub const DEV_ENV: &str = "dev";
pub const PROD_ENV: &str = "prod";

pub fn get_env() -> &'static str {
    ENV.get_or_init(|| {
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| DEV_ENV.to_string()) 
    })
}

pub fn get_symbol() -> &'static str {
    SYMBOL.get_or_init(|| {
        std::env::args()
            .nth(2)
            .unwrap_or_else(|| "BTCUSDT".to_string())
    })
}

static REST_BASEURL: OnceLock<String> = OnceLock::new();

pub fn get_rest_baseurl() -> &'static str {
    REST_BASEURL.get_or_init(|| {
        if get_env() == TEST_ENV || get_env() == DEV_ENV {
            "https://demo-fapi.binance.com".to_string()
        } else if get_env() == PROD_ENV {
            "https://fapi.binance.com".to_string()
        } else {
            "".to_string()
        }
    })
}

pub fn get_ws_baseurl(listen_key: &str) -> String {
    if get_env() == TEST_ENV || get_env() == DEV_ENV {
        format!("wss://fstream.binancefuture.com/private/stream?listenKey={}", listen_key)
    } else if get_env() == PROD_ENV {
        format!("wss://fstream.binance.com/private/stream?listenKey={}", listen_key)
    } else {
        "".to_string()
    }
}

pub fn get_ws_market_baseurl() -> String {
    if get_env() == TEST_ENV || get_env() == DEV_ENV {
        "wss://fstream.binancefuture.com/market/stream".to_string()
    } else if get_env() == PROD_ENV {
        "wss://fstream.binance.com/market/stream".to_string()
    } else {
        "".to_string()
    }
}

static WS_PUBLIC_BASEURL: OnceLock<String> = OnceLock::new();

pub fn get_ws_public_baseurl() -> &'static str {
    WS_PUBLIC_BASEURL.get_or_init(|| {
        if get_env() == TEST_ENV || get_env() == DEV_ENV {
            "wss://fstream.binancefuture.com/public/stream".to_string()
        } else if get_env() == PROD_ENV {
            "wss://fstream.binance.com/public/stream".to_string()
        } else {
            "".to_string()
        }
    })
}

static WS_API_BASEURL: OnceLock<String> = OnceLock::new();

pub fn get_ws_api_baseurl() -> &'static str {
    WS_API_BASEURL.get_or_init(|| {
        if get_env() == TEST_ENV || get_env() == DEV_ENV {
            "wss://testnet.binancefuture.com/ws-fapi/v1".to_string()
        } else if get_env() == PROD_ENV {
            "wss://ws-fapi.binance.com/ws-fapi/v1".to_string()
        } else {
            "".to_string()
        }
    })
}

#[derive(serde::Deserialize)]
struct ServerTimeResponse {
    serverTime: i64,
}

pub async fn get_server_time() -> Result<i64, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let url = format!("{}/fapi/v1/time", get_rest_baseurl());
    let response = client.get(&url).send().await?;
    
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await?;
        return Err(format!("API request failed with status {}: {}", status, body).into());
    }
    
    let body = response.text().await?;
    if body.is_empty() {
        return Err("API returned empty response".into());
    }
    
    let time_response: ServerTimeResponse = serde_json::from_str(&body)?;
    Ok(time_response.serverTime)
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RateLimit {
    pub interval: String,
    pub interval_num: i64,
    pub limit: i64,
    pub rate_limit_type: String,
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub asset: String,
    pub margin_available: bool,
    pub auto_asset_exchange: Option<String>,
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    pub filter_type: String,
    #[serde(default)]
    pub max_price: Option<String>,
    #[serde(default)]
    pub min_price: Option<String>,
    #[serde(default)]
    pub tick_size: Option<String>,
    #[serde(default)]
    pub max_qty: Option<String>,
    #[serde(default)]
    pub min_qty: Option<String>,
    #[serde(default)]
    pub step_size: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub notional: Option<String>,
    #[serde(default)]
    pub multiplier_up: Option<String>,
    #[serde(default)]
    pub multiplier_down: Option<String>,
    #[serde(default)]
    pub multiplier_decimal: Option<String>,
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Symbol {
    pub symbol: String,
    pub pair: String,
    pub contract_type: String,
    #[serde(default)]
    pub delivery_date: i64,
    #[serde(default)]
    pub onboard_date: i64,
    pub status: String,
    pub maint_margin_percent: String,
    pub required_margin_percent: String,
    pub base_asset: String,
    pub quote_asset: String,
    pub margin_asset: String,
    pub price_precision: i32,
    pub quantity_precision: i32,
    pub base_asset_precision: i32,
    pub quote_precision: i32,
    pub underlying_type: String,
    #[serde(default)]
    pub underlying_sub_type: Vec<String>,
    #[serde(default)]
    pub settle_plan: i64,
    pub trigger_protect: String,
    pub filters: Vec<Filter>,
    pub order_types: Vec<String>,
    pub time_in_force: Vec<String>,
    pub liquidation_fee: String,
    pub market_take_bound: String,
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ExchangeInfo {
    pub exchange_filters: Vec<serde_json::Value>,
    pub rate_limits: Vec<RateLimit>,
    pub server_time: i64,
    pub assets: Vec<Asset>,
    pub symbols: Vec<Symbol>,
    pub timezone: String,
}

pub async fn get_exchange_info() -> Result<ExchangeInfo, Box<dyn std::error::Error>> {
    let data_dir = match std::env::var("CARGO_MANIFEST_DIR") {
        Ok(manifest_dir) => std::path::Path::new(&manifest_dir).join("data"),
        Err(_) => std::path::Path::new("data").to_path_buf(),
    };
    
    if !data_dir.exists() {
        std::fs::create_dir_all(&data_dir)?;
    }
    
    let cache_file = data_dir.join("exchange_info.json");
    
    if cache_file.exists() {
        let file_content = std::fs::read_to_string(&cache_file)?;
        if !file_content.is_empty() {
            if let Ok(exchange_info) = serde_json::from_str::<ExchangeInfo>(&file_content) {
                return Ok(exchange_info);
            }
        }
    }
    
    let client = reqwest::Client::new();
    let url = format!("{}/fapi/v1/exchangeInfo", get_rest_baseurl());
    let response = client.get(&url).send().await?;
    
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await?;
        return Err(format!("API request failed with status {}: {}", status, body).into());
    }
    
    let body = response.text().await?;
    if body.is_empty() {
        return Err("API returned empty response".into());
    }
    
    let exchange_info: ExchangeInfo = serde_json::from_str(&body)?;
    
    let json_content = serde_json::to_string_pretty(&exchange_info)?;
    std::fs::write(&cache_file, json_content)?;
    
    Ok(exchange_info)
}
/// 计算给定 USD 金额在某个价格下能购买的数量
///
/// # 参数
/// - `usd_amount`: 投入的 USD 金额，如 "100.5"
/// - `symbol`: 交易对名称，如 "BLZUSDT"（仅用于日志/错误信息）
/// - `price`: 当前市场价格，如 "0.0234"
/// - `filters`: 从交易所 exchangeInfo 解析出的过滤规则
///
/// # 返回
/// Ok(Decimal) 为最终可下单的数量（已对齐 stepSize）
pub fn calc_quantity(
    usd_amount: &str,
    symbol: &str,
    price: &str,
    filters: &Symbol,
) -> Result<Decimal, CalcError> {
    // 1. 解析输入，全程用 Decimal 避免浮点精度问题
    let usd = Decimal::from_str(usd_amount)
        .map_err(|_| CalcError::InvalidInput(format!("Invalid usd_amount: {usd_amount}")))?;
    let px = Decimal::from_str(price)
        .map_err(|_| CalcError::InvalidInput(format!("Invalid price: {price}")))?;

    if usd <= Decimal::ZERO {
        return Err(CalcError::InvalidInput("usd_amount must be > 0".into()));
    }
    if px <= Decimal::ZERO {
        return Err(CalcError::InvalidInput("price must be > 0".into()));
    }

    // 2. 检查价格是否在合法范围内
    if px < filters.min_price || px > filters.max_price {
        return Err(CalcError::PriceOutOfRange(
            px,
            filters.min_price,
            filters.max_price,
        ));
    }

    // 3. 检查最小名义价值 (MIN_NOTIONAL)
    //    名义价值 = qty * price，所以先用 usd_amount 近似检查
    if usd < filters.min_notional {
        return Err(CalcError::BelowMinNotional(usd, filters.min_notional));
    }

    // 4. 计算原始数量
    //    qty = usd_amount / price
    let raw_qty = usd / px;

    // 5. 根据市价单或限价单选择对应的 stepSize 规则
    //    市价单优先用 MARKET_LOT_SIZE，否则用 LOT_SIZE
    let (step_size, min_qty, max_qty) = if let (
        Some(m_step),
        Some(m_min),
        Some(m_max),
    ) = (
        filters.market_lot_size_step_size,
        filters.market_lot_size_min_qty,
        filters.market_lot_size_max_qty,
    ) {
        (m_step, m_min, m_max)
    } else {
        (
            filters.lot_size_step_size,
            filters.lot_size_min_qty,
            filters.lot_size_max_qty,
        )
    };

    // 6. 对齐 stepSize（向下取整，确保不超出可用资金）
    let qty = floor_to_step(raw_qty, step_size);

    // 7. 校验数量范围
    if qty < min_qty {
        return Err(CalcError::BelowMinQty(qty, min_qty));
    }
    if qty > max_qty {
        return Err(CalcError::ExceedsMaxQty(qty, max_qty));
    }

    // 8. 二次校验名义价值（对齐 stepSize 后实际花费可能略低于 usd_amount）
    let actual_notional = qty * px;
    if actual_notional < filters.min_notional {
        return Err(CalcError::BelowMinNotional(actual_notional, filters.min_notional));
    }

    println!(
        "[{symbol}] usd={usd_amount}, price={price}, raw_qty={raw_qty}, \
         step_size={step_size}, final_qty={qty}, actual_cost={actual_notional}"
    );

    Ok(qty)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_exchange_info() {
        ENV.get_or_init(|| DEV_ENV.to_string());
        REST_BASEURL.get_or_init(|| {
            "https://demo-fapi.binance.com".to_string()
        });
        
        let result = get_exchange_info().await;
        assert!(result.is_ok(), "get_exchange_info failed: {:?}", result);
        
        let exchange_info = result.unwrap();
        assert!(!exchange_info.timezone.is_empty());
        assert!(exchange_info.server_time > 0);
        assert!(!exchange_info.symbols.is_empty());
        assert!(!exchange_info.rate_limits.is_empty());
        
        println!("Successfully retrieved exchange info with {} symbols", exchange_info.symbols.len());
        for symbol in exchange_info.symbols {
            println!("Symbol: {}, Pair: {}, Contract Type: {}", symbol.symbol, symbol.pair, symbol.contract_type);
        }
    }
}