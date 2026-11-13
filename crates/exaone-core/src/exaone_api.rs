//! EXAONE API 직접 호출 (게이트웨이를 거치지 않음). MCP 서버가 쓴다.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Result, bail};
use serde_json::{Value, json};

use crate::UPSTREAM_MODEL;

/// 서버 혼잡 429는 무작위로 오므로 짧게 자주 다시 시도한다 (최대 약 1분).
const MAX_ATTEMPTS: u32 = 15;

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub prompt: String,
    pub system: Option<String>,
    pub reasoning_effort: String,
    pub max_tokens: u32,
}

pub fn chat(api_base: &str, api_key: &str, req: &ChatRequest) -> Result<String> {
    let mut messages = Vec::new();
    if let Some(s) = req.system.as_deref().filter(|s| !s.trim().is_empty()) {
        messages.push(json!({"role": "system", "content": s}));
    }
    messages.push(json!({"role": "user", "content": req.prompt}));
    let body = json!({
        "model": UPSTREAM_MODEL,
        "messages": messages,
        "max_tokens": req.max_tokens,
        "temperature": 1.0,
        "top_p": 0.95,
        "reasoning_effort": req.reasoning_effort,
    });

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(600)))
        .http_status_as_error(false)
        .build()
        .into();
    let url = format!("{}/chat/completions", api_base.trim_end_matches('/'));

    for attempt in 1..=MAX_ATTEMPTS {
        let mut resp = agent
            .post(&url)
            .header("Authorization", &format!("Bearer {api_key}"))
            .send_json(&body)?;
        let status = resp.status().as_u16();
        let text = resp.body_mut().read_to_string()?;
        match status {
            200 => {
                let v: Value = serde_json::from_str(&text)?;
                let content = v["choices"][0]["message"]["content"]
                    .as_str()
                    .unwrap_or_default();
                return Ok(content.trim().to_string());
            }
            429 if attempt < MAX_ATTEMPTS => std::thread::sleep(jitter_delay()),
            429 => bail!("EXAONE 호출 한도 초과 또는 서버 혼잡(429). 잠시 뒤 다시 시도하세요."),
            _ => bail!(
                "EXAONE API {status}: {}",
                text.chars().take(500).collect::<String>()
            ),
        }
    }
    unreachable!()
}

/// 2~5초 사이의 대기 시간.
fn jitter_delay() -> Duration {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    Duration::from_millis(2000 + u64::from(nanos % 3000))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jitter_in_range() {
        for _ in 0..50 {
            let d = jitter_delay();
            assert!(d >= Duration::from_secs(2) && d < Duration::from_secs(5));
        }
    }
}
