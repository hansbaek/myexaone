//! EXAONE을 `exaone_chat` 도구로 노출하는 MCP 서버 (stdio, 줄 단위 JSON-RPC).
//!
//! 메인 모델을 바꿀 수 없는 Antigravity 같은 도구, 또는 Claude를 메인으로 쓰면서
//! EXAONE에게 일부를 맡기고 싶을 때 쓴다.

use std::io::{BufRead, Write};

use anyhow::Result;
use serde_json::{Value, json};

use crate::exaone_api::{self, ChatRequest};

const DEFAULT_PROTOCOL: &str = "2025-06-18";

pub const TOOL_NAME: &str = "exaone_chat";
const TOOL_DESCRIPTION: &str = "LG K-EXAONE 2.5에게 질문하거나 작업을 맡기고 답변 텍스트를 받는다. \
한국어 글쓰기·번역·요약·검토, 한국 관련 지식, 다른 모델의 두 번째 의견이 필요할 때 쓴다. \
대화 기록을 기억하지 않으므로 필요한 맥락(코드, 문서 등)을 prompt에 모두 넣는다. \
텍스트만 받는다(이미지 불가). 답변까지 수십 초가 걸릴 수 있다.";

fn tool_schema() -> Value {
    json!({
        "name": TOOL_NAME,
        "description": TOOL_DESCRIPTION,
        "inputSchema": {
            "type": "object",
            "properties": {
                "prompt": {"type": "string", "description": "EXAONE에게 보낼 질문이나 작업 (필요한 맥락 포함)"},
                "system": {"type": "string", "description": "선택: 시스템 지시문"},
                "reasoning_effort": {"type": "string", "enum": ["low", "high", "max"], "default": "high"},
                "max_tokens": {"type": "integer", "default": 8192, "minimum": 1}
            },
            "required": ["prompt"]
        }
    })
}

/// 도구 호출을 실제로 처리하는 함수. 실패하면 사용자에게 보일 메시지를 돌려준다.
pub type ToolHandler<'a> = dyn Fn(&str, &Value) -> std::result::Result<String, String> + 'a;

/// 입력에서 요청을 읽어 처리하고 응답을 쓴다. 입력이 끝나면 돌아온다.
pub fn serve(input: impl BufRead, mut output: impl Write, handler: &ToolHandler) -> Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(resp) = handle_line(&line, handler) {
            writeln!(output, "{resp}")?;
            output.flush()?;
        }
    }
    Ok(())
}

fn handle_line(line: &str, handler: &ToolHandler) -> Option<Value> {
    let msg: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return Some(error(Value::Null, -32700, "Parse error")),
    };
    // id 가 없으면 알림(notification)이므로 응답하지 않는다.
    let id = msg.get("id").cloned()?;
    let method = msg
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let params = msg.get("params").cloned().unwrap_or(Value::Null);

    let result = match method {
        "initialize" => json!({
            "protocolVersion": params.get("protocolVersion").and_then(Value::as_str).unwrap_or(DEFAULT_PROTOCOL),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "exaone", "version": env!("CARGO_PKG_VERSION")},
        }),
        "ping" => json!({}),
        "tools/list" => json!({"tools": [tool_schema()]}),
        "tools/call" => {
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if name != TOOL_NAME {
                return Some(error(id, -32602, &format!("알 수 없는 도구: {name}")));
            }
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            match handler(name, &args) {
                Ok(text) => json!({"content": [{"type": "text", "text": text}], "isError": false}),
                Err(msg) => {
                    json!({"content": [{"type": "text", "text": format!("오류: {msg}")}], "isError": true})
                }
            }
        }
        _ => {
            return Some(error(
                id,
                -32601,
                &format!("지원하지 않는 메서드: {method}"),
            ));
        }
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// `exaone_chat` 인자를 EXAONE 요청으로 바꾼다.
pub fn parse_chat_args(args: &Value) -> std::result::Result<ChatRequest, String> {
    let prompt = args
        .get("prompt")
        .and_then(Value::as_str)
        .filter(|p| !p.trim().is_empty())
        .ok_or("prompt 가 비어 있습니다")?;
    let effort = args
        .get("reasoning_effort")
        .and_then(Value::as_str)
        .unwrap_or("high");
    if !["low", "high", "max"].contains(&effort) {
        return Err(format!(
            "reasoning_effort 는 low/high/max 중 하나여야 합니다: {effort}"
        ));
    }
    Ok(ChatRequest {
        prompt: prompt.to_string(),
        system: args
            .get("system")
            .and_then(Value::as_str)
            .map(str::to_string),
        reasoning_effort: effort.to_string(),
        max_tokens: args
            .get("max_tokens")
            .and_then(Value::as_u64)
            .map_or(8192, |n| n.clamp(1, 65536) as u32),
    })
}

/// 표준 입출력으로 MCP 서버를 실행한다.
pub fn run_stdio(api_base: &str, api_key: Option<String>) -> Result<()> {
    let handler = |_: &str, args: &Value| {
        let key = api_key
            .as_deref()
            .ok_or("EXAONE API 키가 없습니다. `exaone key set` 으로 저장하세요.")?;
        let req = parse_chat_args(args)?;
        exaone_api::chat(api_base, key, &req).map_err(|e| e.to_string())
    };
    serve(std::io::stdin().lock(), std::io::stdout().lock(), &handler)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(lines: &[Value], handler: &ToolHandler) -> Vec<Value> {
        let input = lines
            .iter()
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let mut out = Vec::new();
        serve(input.as_bytes(), &mut out, handler).unwrap();
        String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    fn echo(_: &str, args: &Value) -> std::result::Result<String, String> {
        parse_chat_args(args).map(|r| format!("echo:{}", r.prompt))
    }

    #[test]
    fn handshake_list_and_call() {
        let out = run(
            &[
                json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}),
                json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
                json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
                json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"exaone_chat","arguments":{"prompt":"안녕"}}}),
            ],
            &echo,
        );
        assert_eq!(out.len(), 3, "알림에는 응답하지 않는다");
        assert_eq!(out[0]["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(out[1]["result"]["tools"][0]["name"], TOOL_NAME);
        assert_eq!(out[2]["id"], 3);
        assert_eq!(out[2]["result"]["content"][0]["text"], "echo:안녕");
        assert_eq!(out[2]["result"]["isError"], false);
    }

    #[test]
    fn tool_errors_are_reported_as_results() {
        let out = run(
            &[
                json!({"jsonrpc":"2.0","id":"a","method":"tools/call","params":{"name":"exaone_chat","arguments":{}}}),
            ],
            &echo,
        );
        assert_eq!(out[0]["result"]["isError"], true);
        assert!(
            out[0]["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("prompt")
        );
    }

    #[test]
    fn unknown_method_and_bad_json() {
        let input = format!(
            "{}\nnot json\n",
            json!({"jsonrpc":"2.0","id":7,"method":"resources/list"})
        );
        let mut out = Vec::new();
        serve(input.as_bytes(), &mut out, &echo).unwrap();
        let v: Vec<Value> = String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(v[0]["error"]["code"], -32601);
        assert_eq!(v[1]["error"]["code"], -32700);
    }

    #[test]
    fn parse_args_validates_effort_and_clamps_tokens() {
        assert!(parse_chat_args(&json!({"prompt":"x","reasoning_effort":"medium"})).is_err());
        let r = parse_chat_args(&json!({"prompt":"x","max_tokens":999999})).unwrap();
        assert_eq!(r.max_tokens, 65536);
        assert_eq!(r.reasoning_effort, "high");
    }
}
