use crate::utils::{get_rest_baseurl, get_http_client};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum KlineInterval {
    #[serde(rename = "1m")]
    M1,
    #[serde(rename = "3m")]
    M3,
    #[serde(rename = "5m")]
    M5,
    #[serde(rename = "15m")]
    M15,
    #[serde(rename = "30m")]
    M30,
    #[serde(rename = "1h")]
    H1,
    #[serde(rename = "2h")]
    H2,
    #[serde(rename = "4h")]
    H4,
    #[serde(rename = "6h")]
    H6,
    #[serde(rename = "8h")]
    H8,
    #[serde(rename = "12h")]
    H12,
    #[serde(rename = "1d")]
    D1,
    #[serde(rename = "3d")]
    D3,
    #[serde(rename = "1w")]
    W1,
    #[serde(rename = "1M")]
    MO1,
}

impl KlineInterval {
    pub fn as_str(&self) -> &'static str {
        match self {
            KlineInterval::M1 => "1m",
            KlineInterval::M3 => "3m",
            KlineInterval::M5 => "5m",
            KlineInterval::M15 => "15m",
            KlineInterval::M30 => "30m",
            KlineInterval::H1 => "1h",
            KlineInterval::H2 => "2h",
            KlineInterval::H4 => "4h",
            KlineInterval::H6 => "6h",
            KlineInterval::H8 => "8h",
            KlineInterval::H12 => "12h",
            KlineInterval::D1 => "1d",
            KlineInterval::D3 => "3d",
            KlineInterval::W1 => "1w",
            KlineInterval::MO1 => "1M",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Kline {
    pub open_time: i64,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
    pub close_time: i64,
    pub quote_volume: Decimal,
    pub trades: i64,
    pub taker_buy_base_volume: Decimal,
    pub taker_buy_quote_volume: Decimal,
}

#[derive(Debug)]
pub enum KlineError {
    RequestFailed(String),
    ParseFailed(String),
}

impl std::fmt::Display for KlineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KlineError::RequestFailed(msg) => write!(f, "RequestFailed: {}", msg),
            KlineError::ParseFailed(msg) => write!(f, "ParseFailed: {}", msg),
        }
    }
}

impl std::error::Error for KlineError {}

pub struct KlineRequest {
    pub symbol: String,
    pub interval: KlineInterval,
    pub start_time: Option<i64>,
    pub end_time: Option<i64>,
    pub limit: Option<u32>,
}

pub async fn get_klines(req: KlineRequest) -> Result<Vec<Kline>, KlineError> {
    let mut params = vec![
        ("symbol".to_string(), req.symbol.clone()),
        ("interval".to_string(), req.interval.as_str().to_string()),
    ];

    if let Some(start_time) = req.start_time {
        params.push(("startTime".to_string(), start_time.to_string()));
    }
    if let Some(end_time) = req.end_time {
        params.push(("endTime".to_string(), end_time.to_string()));
    }
    if let Some(limit) = req.limit {
        params.push(("limit".to_string(), limit.to_string()));
    }

    let query: Vec<(String, String)> = params;
    let url = format!("{}/fapi/v1/klines", get_rest_baseurl());

    let client = get_http_client();
    let response = client
        .get(&url)
        .query(&query)
        .send()
        .await
        .map_err(|e| KlineError::RequestFailed(e.to_string()))?;

    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<empty>".to_string());
        return Err(KlineError::RequestFailed(format!(
            "API request failed with status {}: {}",
            status, body
        )));
    }

    let body = response
        .text()
        .await
        .map_err(|e| KlineError::RequestFailed(e.to_string()))?;

    if body.is_empty() {
        return Err(KlineError::ParseFailed("API returned empty response".into()));
    }

    let raw: Vec<Vec<serde_json::Value>> =
        serde_json::from_str(&body).map_err(|e| KlineError::ParseFailed(e.to_string()))?;

    let mut klines = Vec::with_capacity(raw.len());
    for item in &raw {
        if item.len() < 11 {
            return Err(KlineError::ParseFailed(format!(
                "Unexpected kline array length: {}",
                item.len()
            )));
        }

        let open_time = item[0]
            .as_i64()
            .ok_or_else(|| KlineError::ParseFailed("Failed to parse open_time".into()))?;
        let open = Decimal::from_str(item[1].as_str().unwrap_or("0"))
            .map_err(|e| KlineError::ParseFailed(format!("Failed to parse open: {}", e)))?;
        let high = Decimal::from_str(item[2].as_str().unwrap_or("0"))
            .map_err(|e| KlineError::ParseFailed(format!("Failed to parse high: {}", e)))?;
        let low = Decimal::from_str(item[3].as_str().unwrap_or("0"))
            .map_err(|e| KlineError::ParseFailed(format!("Failed to parse low: {}", e)))?;
        let close = Decimal::from_str(item[4].as_str().unwrap_or("0"))
            .map_err(|e| KlineError::ParseFailed(format!("Failed to parse close: {}", e)))?;
        let volume = Decimal::from_str(item[5].as_str().unwrap_or("0"))
            .map_err(|e| KlineError::ParseFailed(format!("Failed to parse volume: {}", e)))?;
        let close_time = item[6]
            .as_i64()
            .ok_or_else(|| KlineError::ParseFailed("Failed to parse close_time".into()))?;
        let quote_volume = Decimal::from_str(item[7].as_str().unwrap_or("0"))
            .map_err(|e| KlineError::ParseFailed(format!("Failed to parse quote_volume: {}", e)))?;
        let trades = item[8]
            .as_i64()
            .ok_or_else(|| KlineError::ParseFailed("Failed to parse trades".into()))?;
        let taker_buy_base_volume = Decimal::from_str(item[9].as_str().unwrap_or("0"))
            .map_err(|e| {
                KlineError::ParseFailed(format!("Failed to parse taker_buy_base_volume: {}", e))
            })?;
        let taker_buy_quote_volume = Decimal::from_str(item[10].as_str().unwrap_or("0"))
            .map_err(|e| {
                KlineError::ParseFailed(format!("Failed to parse taker_buy_quote_volume: {}", e))
            })?;

        klines.push(Kline {
            open_time,
            open,
            high,
            low,
            close,
            volume,
            close_time,
            quote_volume,
            trades,
            taker_buy_base_volume,
            taker_buy_quote_volume,
        });
    }

    Ok(klines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn get_server_time(){
        let client = get_http_client();
        let url = format!("{}", get_rest_baseurl());
        let response = client.get(&url).send().await.unwrap();

        println!("{:#?}", response);

    }
    #[tokio::test]
    async fn test_get_klines() {
        let req = KlineRequest {
            symbol: "ETHUSDC".to_string(),
            interval: KlineInterval::M1,
            start_time: None,
            end_time: None,
            limit: Some(5),
        };

        let result = get_klines(req).await;
        assert!(result.is_ok(), "get_klines failed: {:?}", result);

        let klines = result.unwrap();
        assert!(!klines.is_empty(), "klines should not be empty");
        assert!(klines.len() <= 5, "klines length should be <= limit");

        let first = &klines[0];
        println!(
            "open_time={}, open={}, high={}, low={}, close={}, volume={}",
            first.open_time, first.open, first.high, first.low, first.close, first.volume
        );
    }
}
