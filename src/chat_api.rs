use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub reasoning_content: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatChoice {
    pub index: u32,
    pub message: ChatMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<ChatChoice>,
    pub usage: Usage,
}

#[derive(Debug)]
pub enum ChatError {
    RequestFailed(String),
    ParseFailed(String),
    ApiError(String),
}

impl std::fmt::Display for ChatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChatError::RequestFailed(msg) => write!(f, "RequestFailed: {}", msg),
            ChatError::ParseFailed(msg) => write!(f, "ParseFailed: {}", msg),
            ChatError::ApiError(msg) => write!(f, "ApiError: {}", msg),
        }
    }
}

impl std::error::Error for ChatError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TradeSignal {
    Buy,
    Sell,
}

#[derive(Debug)]
pub enum SignalError {
    KlineError(String),
    ChatError(ChatError),
    InvalidSignal(String),
}

impl std::fmt::Display for SignalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SignalError::KlineError(msg) => write!(f, "KlineError: {}", msg),
            SignalError::ChatError(e) => write!(f, "ChatError: {}", e),
            SignalError::InvalidSignal(msg) => write!(f, "InvalidSignal: {}", msg),
        }
    }
}

impl std::error::Error for SignalError {}

pub async fn deepseek_kline_analysis(api_key: &str) -> Result<TradeSignal, SignalError> {
    let symbol = crate::utils::get_symbol();

    let kline_req = crate::rest_api::KlineRequest {
        symbol: symbol.to_string(),
        interval: crate::rest_api::KlineInterval::M15,
        start_time: None,
        end_time: None,
        limit: Some(96),
    };

    let klines = crate::rest_api::get_klines(kline_req)
        .await
        .map_err(|e| SignalError::KlineError(e.to_string()))?;

    let mut kline_text = String::new();
    for k in &klines {
        kline_text.push_str(&format!(
            "时间:{}, 开:{}, 高:{}, 低:{}, 收:{}, 量:{}, 引用量:{}\n",
            k.open_time, k.open, k.high, k.low, k.close, k.volume, k.quote_volume
        ));
    }

    let system_prompt = format!(
        "你是一个专业的加密货币K线数据分析专家。你将收到{}交易对最近96条15分钟K线数据。\
        请基于技术分析（包括趋势、支撑阻力、成交量、价格形态等）判断当前市场走势。\
        你只能回复一个字：买 或 卖。不要回复任何其他内容，不要解释原因，只回复买或卖。",
        symbol
    );

    let user_prompt = format!("以下是{}最近96条15分钟K线数据：\n\n{}", symbol, kline_text);

    let req = ChatRequest {
        base_url: "https://api.deepseek.com".to_string(),
        api_key: api_key.to_string(),
        model: "deepseek-reasoner".to_string(),
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: system_prompt,
                reasoning_content: None,
            },
            ChatMessage {
                role: "user".to_string(),
                content: user_prompt,
                reasoning_content: None,
            },
        ],
        temperature: Some(1.0),
        max_tokens: None,
        stream: false,
    };

    let response = chat_completion(req)
        .await
        .map_err(SignalError::ChatError)?;

    let first_choice = response
        .choices
        .first()
        .ok_or_else(|| SignalError::InvalidSignal("No choices in response".into()))?;

    if let Some(reasoning) = &first_choice.message.reasoning_content {
        println!("=== DeepSeek 思考过程 ===\n{}\n=== 思考过程结束 ===", reasoning);
    }

    let choice = response.choices.first()
        .ok_or_else(|| SignalError::InvalidSignal("No choices".into()))?;

    let content = {
        let c = choice.message.content.trim();
        if c.is_empty() {
            choice.message.reasoning_content
                .as_deref()
                .unwrap_or("")
                .trim()
        } else {
            c
        }
    };

    if content.contains("买") {
        Ok(TradeSignal::Buy)
    } else if content.contains("卖") {
        Ok(TradeSignal::Sell)
    } else {
        Err(SignalError::InvalidSignal(format!(
            "模型返回了无效信号: '{}', 期望 '买' 或 '卖'",
            content
        )))
    }
}

pub async fn chat_completion(req: ChatRequest) -> Result<ChatResponse, ChatError> {
    let url = format!("{}/chat/completions", req.base_url.trim_end_matches('/'));

    let mut body = serde_json::json!({
        "model": req.model,
        "messages": req.messages,
        "stream": req.stream,
    });

    if let Some(temperature) = req.temperature {
        body["temperature"] = serde_json::json!(temperature);
    }
    if let Some(max_tokens) = req.max_tokens {
        body["max_tokens"] = serde_json::json!(max_tokens);
    }

    let client = crate::utils::get_http_client();
    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", req.api_key))
        .json(&body)
        .send()
        .await
        .map_err(|e| ChatError::RequestFailed(e.to_string()))?;

    let status = response.status();
    if !status.is_success() {
        let error_body = response
            .text()
            .await
            .unwrap_or_else(|_| "<empty>".to_string());
        return Err(ChatError::ApiError(format!(
            "API request failed with status {}: {}",
            status, error_body
        )));
    }

    let response_text = response
        .text()
        .await
        .map_err(|e| ChatError::RequestFailed(e.to_string()))?;

    if response_text.is_empty() {
        return Err(ChatError::ParseFailed("API returned empty response".into()));
    }

    let chat_response: ChatResponse =
        serde_json::from_str(&response_text).map_err(|e| {
            ChatError::ParseFailed(format!("Failed to parse response: {} - Body: {}", e, response_text))
        })?;

    Ok(chat_response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_deepseek_kline_analysis() {
        let api_key = std::env::var("DEEPSEEK_API_KEY")
            .expect("DEEPSEEK_API_KEY environment variable not set");
        println!("{:?}", deepseek_kline_analysis(api_key.as_str()).await);
    }

    #[tokio::test]
    async fn test_chat_completion_deepseek() {
        let api_key = std::env::var("DEEPSEEK_API_KEY")
            .expect("DEEPSEEK_API_KEY environment variable not set");

        let req = ChatRequest {
            base_url: "https://api.deepseek.com".to_string(),
            api_key,
            model: "deepseek-chat".to_string(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: "You are a helpful assistant.".to_string(),
                    reasoning_content: None,
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: "Hello!".to_string(),
                    reasoning_content: None,
                },
            ],
            temperature: None,
            max_tokens: None,
            stream: false,
        };

        let result = chat_completion(req).await;
        match result {
            Ok(resp) => {
                println!("Response: {:?}", resp);
                println!("{}", resp.choices[0].message.content);
                assert!(!resp.choices.is_empty());
                assert_eq!(resp.choices[0].message.role, "assistant");
            }
            Err(e) => panic!("chat_completion failed: {}", e),
        }
    }
}


