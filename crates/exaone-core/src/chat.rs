//! 게이트웨이를 거쳐 EXAONE과 대화한다 (스트리밍). 앱의 대화 창이 쓴다.

use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::GATEWAY_MODEL;

/// 스트리밍 중에 생기는 일.
#[derive(Debug, Clone, PartialEq)]
pub enum ChatEvent {
    Reasoning(String),
    Content(String),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Usage {
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct ChatTurn {
    pub role: String,
    pub content: String,
}

#[derive(Deserialize)]
struct Chunk {
    #[serde(default)]
    choices: Vec<ChunkChoice>,
    usage: Option<Value>,
}

#[derive(Deserialize)]
struct ChunkChoice {
    #[serde(default)]
    delta: Delta,
}

#[derive(Deserialize, Default)]
struct Delta {
    content: Option<String>,
    reasoning_content: Option<String>,
}

fn int(v: &Value, path: &[&str]) -> Option<i64> {
    let mut cur = v;
    for p in path {
        cur = cur.get(p)?;
    }
    cur.as_i64()
}

/// SSE 한 줄을 해석한다. `None` 이면 무시할 줄.
fn parse_line(line: &str, on: &mut dyn FnMut(ChatEvent), usage: &mut Usage) -> Option<()> {
    let data = line.strip_prefix("data:")?.trim();
    if data == "[DONE]" || data.is_empty() {
        return None;
    }
    let chunk: Chunk = serde_json::from_str(data).ok()?;
    for c in chunk.choices {
        if let Some(r) = c.delta.reasoning_content.filter(|s| !s.is_empty()) {
            on(ChatEvent::Reasoning(r));
        }
        if let Some(t) = c.delta.content.filter(|s| !s.is_empty()) {
            on(ChatEvent::Content(t));
        }
    }
    if let Some(u) = chunk.usage {
        usage.prompt_tokens = int(&u, &["prompt_tokens"]).or(usage.prompt_tokens);
        usage.completion_tokens = int(&u, &["completion_tokens"]).or(usage.completion_tokens);
        usage.reasoning_tokens = int(&u, &["completion_tokens_details", "reasoning_tokens"])
            .or_else(|| int(&u, &["reasoning_tokens"]))
            .or(usage.reasoning_tokens);
    }
    Some(())
}

/// 대화를 보내고 답변을 스트리밍으로 받는다. `cancel` 이 참이 되면 멈춘다(지금까지 받은 것은 유지).
pub fn stream(
    gateway_url: &str,
    gateway_key: &str,
    history: &[ChatTurn],
    reasoning_effort: &str,
    cancel: &AtomicBool,
    on: &mut dyn FnMut(ChatEvent),
) -> Result<Usage> {
    let messages: Vec<Value> = history
        .iter()
        .map(|t| json!({"role": t.role, "content": t.content}))
        .collect();
    let body = json!({
        "model": GATEWAY_MODEL,
        "messages": messages,
        "stream": true,
        "stream_options": {"include_usage": true},
        "reasoning_effort": reasoning_effort,
    });
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(5)))
        .timeout_recv_body(Some(Duration::from_secs(600)))
        .http_status_as_error(false)
        .build()
        .into();
    let resp = agent
        .post(&format!("{gateway_url}/v1/chat/completions"))
        .header("Authorization", &format!("Bearer {gateway_key}"))
        .send_json(&body)?;
    let status = resp.status().as_u16();
    let mut reader = BufReader::new(resp.into_body().into_reader());
    if status != 200 {
        let mut text = String::new();
        let _ = std::io::Read::read_to_string(&mut reader, &mut text);
        bail!("게이트웨이 {status}: {}", error_message(&text));
    }
    let mut usage = Usage::default();
    let mut line = String::new();
    loop {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        parse_line(line.trim_end(), on, &mut usage);
    }
    Ok(usage)
}

/// 오류 응답 본문에서 사람이 읽을 메시지를 꺼낸다.
fn error_message(text: &str) -> String {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
        .unwrap_or_else(|| text.chars().take(300).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_reasoning_content_and_usage() {
        let mut ev = Vec::new();
        let mut usage = Usage::default();
        let mut on = |e| ev.push(e);
        let lines = [
            r#"data: {"choices":[{"delta":{"role":"assistant","content":""}}]}"#,
            r#"data: {"choices":[{"delta":{"reasoning_content":"생각"}}]}"#,
            r#"data: {"choices":[{"delta":{"content":"\n\n답"}}]}"#,
            r#"data: {"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":7,"completion_tokens_details":{"reasoning_tokens":3}}}"#,
            "data: [DONE]",
            ": keep-alive",
        ];
        for l in lines {
            parse_line(l, &mut on, &mut usage);
        }
        assert_eq!(
            ev,
            vec![
                ChatEvent::Reasoning("생각".into()),
                ChatEvent::Content("\n\n답".into())
            ]
        );
        assert_eq!(
            usage,
            Usage {
                prompt_tokens: Some(10),
                completion_tokens: Some(7),
                reasoning_tokens: Some(3)
            }
        );
    }

    #[test]
    fn error_message_prefers_json_message() {
        assert_eq!(error_message(r#"{"error":{"message":"busy"}}"#), "busy");
        assert_eq!(error_message("plain"), "plain");
    }
}
