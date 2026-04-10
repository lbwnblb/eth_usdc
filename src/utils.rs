use std::sync::OnceLock;
use std::str::FromStr;
use rust_decimal::Decimal;

static ENV: OnceLock<String> = OnceLock::new();
static SYMBOL: OnceLock<String> = OnceLock::new();
static DEEPSEEK_API_KEY: OnceLock<String> = OnceLock::new();

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
        "ETHUSDC".to_string()
    })
}

pub fn get_deepseek_api_key() -> &'static str {
    DEEPSEEK_API_KEY.get_or_init(|| {
        std::env::var("DEEPSEEK_API_KEY").unwrap_or_else(|_| String::new())
    })
}

static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

pub fn get_http_client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .build()
            .expect("Failed to build HTTP client")
    })
}

static REST_BASEURL: OnceLock<String> = OnceLock::new();

pub fn get_rest_baseurl() -> &'static str {
    REST_BASEURL.get_or_init(|| {
        //测试用不了暂时
        // if get_env() == TEST_ENV || get_env() == DEV_ENV {
        //     "https://demo-fapi.binance.com".to_string()
        // } else if get_env() == PROD_ENV {
            "https://fapi.binance.com".to_string()
        // } else {
        //     "".to_string()
        // }
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
    // if get_env() == PROD_ENV {
    //     let local_time = std::time::SystemTime::now()
    //         .duration_since(std::time::UNIX_EPOCH)
    //         .unwrap()
    //         .as_millis() as i64;
    //     return Ok(local_time);
    // }
    let client = get_http_client();
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

    let client = get_http_client();
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
/// calc_quantity 可能返回的错误类型
#[derive(Debug)]
pub enum CalcError {
    InvalidInput(String),
    PriceOutOfRange(Decimal, Decimal, Decimal),
    BelowMinNotional(Decimal, Decimal),
    BelowMinQty(Decimal, Decimal),
    ExceedsMaxQty(Decimal, Decimal),
    MissingFilter(String),
}

impl std::fmt::Display for CalcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CalcError::InvalidInput(msg) => write!(f, "InvalidInput: {}", msg),
            CalcError::PriceOutOfRange(px, min, max) => {
                write!(f, "PriceOutOfRange: {} not in [{}, {}]", px, min, max)
            }
            CalcError::BelowMinNotional(actual, min) => {
                write!(f, "BelowMinNotional: {} < {}", actual, min)
            }
            CalcError::BelowMinQty(qty, min) => write!(f, "BelowMinQty: {} < {}", qty, min),
            CalcError::ExceedsMaxQty(qty, max) => write!(f, "ExceedsMaxQty: {} > {}", qty, max),
            CalcError::MissingFilter(name) => write!(f, "MissingFilter: {} not found", name),
        }
    }
}

impl std::error::Error for CalcError {}

/// 将 value 向下对齐到 step_size 的整数倍
/// 例如 floor_to_step(0.123, 0.01) => 0.12
fn floor_to_step(value: Decimal, step_size: Decimal) -> Decimal {
    if step_size <= Decimal::ZERO {
        return value;
    }
    (value / step_size).floor() * step_size
}

/// 从 Symbol.filters 列表中按 filter_type 找到某个过滤器
fn find_filter<'a>(filters: &'a [Filter], filter_type: &str) -> Option<&'a Filter> {
    filters.iter().find(|f| f.filter_type == filter_type)
}

/// 解析 Filter 中的字符串字段为 Decimal，字段不存在或解析失败均返回 None
fn parse_field(opt: &Option<String>) -> Option<Decimal> {
    opt.as_deref().and_then(|s| Decimal::from_str(s).ok())
}

/// 计算给定 USD 金额在某个价格下能购买的数量
///
/// # 参数
/// - `usd_amount`: 投入的 USD 金额，如 "100.5"
/// - `symbol`: 交易对名称，如 "BLZUSDT"（仅用于日志/错误信息）
/// - `price`: 当前市场价格，如 "0.0234"
/// - `symbol_info`: 从交易所 exchangeInfo 解析出的 Symbol（包含 filters）
///
/// # 返回
/// Ok(Decimal) 为最终可下单的数量（已对齐 stepSize）
pub fn calc_quantity(
    usd_amount: &str,
    _symbol: &str,
    price: &str,
    symbol_info: &Symbol,
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

    let filters = &symbol_info.filters;

    // 2. 检查价格是否在合法范围内 (PRICE_FILTER)
    if let Some(pf) = find_filter(filters, "PRICE_FILTER") {
        let min_price = parse_field(&pf.min_price).unwrap_or(Decimal::ZERO);
        let max_price = parse_field(&pf.max_price).unwrap_or(Decimal::MAX);
        if px < min_price || px > max_price {
            return Err(CalcError::PriceOutOfRange(px, min_price, max_price));
        }
    }

    // 3. 检查最小名义价值 (MIN_NOTIONAL)
    let min_notional = find_filter(filters, "MIN_NOTIONAL")
        .and_then(|f| parse_field(&f.notional))
        .unwrap_or(Decimal::ZERO);

    if usd < min_notional {
        return Err(CalcError::BelowMinNotional(usd, min_notional));
    }

    // 4. 计算原始数量：qty = usd_amount / price
    let raw_qty = usd / px;

    // 5. 优先使用 MARKET_LOT_SIZE，否则回退到 LOT_SIZE
    let (step_size, min_qty, max_qty) = {
        let market = find_filter(filters, "MARKET_LOT_SIZE").and_then(|f| {
            let step = parse_field(&f.step_size)?;
            let min = parse_field(&f.min_qty)?;
            let max = parse_field(&f.max_qty)?;
            // step_size 为 0 表示无限制，视为未设置
            if step == Decimal::ZERO { None } else { Some((step, min, max)) }
        });

        if let Some(m) = market {
            m
        } else {
            let lot = find_filter(filters, "LOT_SIZE")
                .ok_or_else(|| CalcError::MissingFilter("LOT_SIZE".into()))?;
            let step = parse_field(&lot.step_size)
                .ok_or_else(|| CalcError::MissingFilter("LOT_SIZE.step_size".into()))?;
            let min = parse_field(&lot.min_qty)
                .ok_or_else(|| CalcError::MissingFilter("LOT_SIZE.min_qty".into()))?;
            let max = parse_field(&lot.max_qty)
                .ok_or_else(|| CalcError::MissingFilter("LOT_SIZE.max_qty".into()))?;
            (step, min, max)
        }
    };

    // 6. 对齐 stepSize（向下取整，确保不超出可用资金）
    let qty = floor_to_step(raw_qty, step_size);

    // 7. 校验数量范围
    if qty < min_qty {
        return Err(CalcError::BelowMinQty(qty, min_qty));
    }
    if max_qty > Decimal::ZERO && qty > max_qty {
        return Err(CalcError::ExceedsMaxQty(qty, max_qty));
    }

    // 8. 二次校验名义价值（对齐 stepSize 后实际花费可能略低于 usd_amount）
    let actual_notional = qty * px;
    if actual_notional < min_notional {
        return Err(CalcError::BelowMinNotional(actual_notional, min_notional));
    }
    Ok(qty)
}
#[cfg(test)]
mod tests {
    use super::*;



    async fn get_symbol_from_api(symbol_name: &str) -> Symbol {
        ENV.get_or_init(|| DEV_ENV.to_string());
        REST_BASEURL.get_or_init(|| {
            "https://demo-fapi.binance.com".to_string()
        });

        let exchange_info = get_exchange_info().await.unwrap();
        exchange_info
            .symbols
            .into_iter()
            .find(|s| s.symbol == symbol_name)
            .unwrap()
    }

    #[tokio::test]
    async fn test_calc_quantity_normal_case() {
        let symbol_info = get_symbol_from_api("ETHUSDT").await;
        let result = calc_quantity("21", "ETHUSDT", "2311.12", &symbol_info);
        println!("{:?}", result)
    }

    #[tokio::test]
    async fn test_calc_quantity_step_size_alignment() {
        let symbol_info = get_symbol_from_api("ETHUSDT").await;
        let result = calc_quantity("150.0", "ETHUSDT", "2000.0", &symbol_info);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_calc_quantity_floor_to_step() {
        let symbol_info = get_symbol_from_api("ETHUSDT").await;
        let result = calc_quantity("100.123", "ETHUSDT", "2000.0", &symbol_info);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_calc_quantity_invalid_usd_amount() {
        let symbol_info = get_symbol_from_api("ETHUSDT").await;
        let result = calc_quantity("invalid", "ETHUSDT", "2000.0", &symbol_info);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CalcError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn test_calc_quantity_invalid_price() {
        let symbol_info = get_symbol_from_api("ETHUSDT").await;
        let result = calc_quantity("100.0", "ETHUSDT", "invalid", &symbol_info);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CalcError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn test_calc_quantity_zero_usd_amount() {
        let symbol_info = get_symbol_from_api("ETHUSDT").await;
        let result = calc_quantity("0", "ETHUSDT", "2000.0", &symbol_info);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CalcError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn test_calc_quantity_zero_price() {
        let symbol_info = get_symbol_from_api("ETHUSDT").await;
        let result = calc_quantity("100.0", "ETHUSDT", "0", &symbol_info);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CalcError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn test_calc_quantity_below_min_notional() {
        let symbol_info = get_symbol_from_api("ETHUSDT").await;
        let result = calc_quantity("5.0", "ETHUSDT", "2000.0", &symbol_info);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_calc_quantity_below_min_qty() {
        let mut symbol_info = get_symbol_from_api("ETHUSDT").await;
        for filter in &mut symbol_info.filters {
            if filter.filter_type == "LOT_SIZE" {
                filter.min_qty = Some("0.1".to_string());
            }
        }
        let result = calc_quantity("100.0", "ETHUSDT", "2000.0", &symbol_info);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CalcError::BelowMinQty(_, _)));
    }

    #[tokio::test]
    async fn test_calc_quantity_exceeds_max_qty() {
        let mut symbol_info = get_symbol_from_api("ETHUSDT").await;
        for filter in &mut symbol_info.filters {
            if filter.filter_type == "LOT_SIZE" {
                filter.max_qty = Some("0.01".to_string());
            }
        }
        let result = calc_quantity("100.0", "ETHUSDT", "2000.0", &symbol_info);
        println!("result: {:?}", result)
    }

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