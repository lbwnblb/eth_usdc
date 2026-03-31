use std::sync::OnceLock;

static ENV: OnceLock<String> = OnceLock::new();

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