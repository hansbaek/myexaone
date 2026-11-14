//! Claude Code·Codex 에 exaone_chat MCP 서버를 등록·해제하고 상태를 확인한다.
//!
//! 두 도구의 공식 명령(`claude mcp add/remove`, `codex mcp add/remove`)을 쓴다.
//! Claude 는 사용자 범위(--scope user)로 등록해 모든 프로젝트에서 쓰게 한다.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::Value;

use crate::launch::{augmented_path, find_program};
use crate::process;

/// 등록할 MCP 서버 이름.
pub const SERVER_NAME: &str = "exaone";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Client {
    Claude,
    Codex,
}

impl Client {
    pub fn program(self) -> &'static str {
        match self {
            Client::Claude => "claude",
            Client::Codex => "codex",
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct McpStatus {
    /// 도구(claude/codex) 명령을 찾았는지
    pub installed: bool,
    /// exaone 이 등록돼 있는지
    pub registered: bool,
    /// 등록된 명령 (등록돼 있으면)
    pub command: Option<Vec<String>>,
    /// 등록된 명령이 지금 쓰려는 명령과 같은지 (앱을 옮기면 달라진다)
    pub up_to_date: bool,
}

fn run(program: &Path, args: &[&str]) -> Result<std::process::Output> {
    let mut cmd = Command::new(program);
    cmd.args(args).env("PATH", augmented_path(program.parent()));
    process::background(&mut cmd);
    cmd.output()
        .with_context(|| format!("{} 실행 실패", program.display()))
}

fn claude_config() -> Option<PathBuf> {
    let dir = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from);
    match dir {
        Some(d) => Some(d.join(".claude.json")),
        None => dirs::home_dir().map(|h| h.join(".claude.json")),
    }
}

/// `~/.claude.json` 의 사용자 범위 MCP 서버에서 exaone 명령을 찾는다.
pub fn claude_registered_command(config_json: &str) -> Option<Vec<String>> {
    let v: Value = serde_json::from_str(config_json).ok()?;
    let s = v.get("mcpServers")?.get(SERVER_NAME)?;
    command_from(s)
}

fn command_from(s: &Value) -> Option<Vec<String>> {
    let mut out = vec![s.get("command")?.as_str()?.to_string()];
    if let Some(args) = s.get("args").and_then(Value::as_array) {
        out.extend(args.iter().filter_map(|a| a.as_str().map(str::to_string)));
    }
    Some(out)
}

/// `codex mcp get exaone --json` 출력에서 명령을 꺼낸다.
pub fn codex_registered_command(json: &str) -> Option<Vec<String>> {
    let v: Value = serde_json::from_str(json).ok()?;
    command_from(v.get("transport").unwrap_or(&v))
}

pub fn status(client: Client, expected: &[String]) -> McpStatus {
    let program = find_program(client.program());
    let command = match (client, &program) {
        (_, None) => None,
        (Client::Claude, Some(_)) => claude_config()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| claude_registered_command(&t)),
        (Client::Codex, Some(p)) => run(p, &["mcp", "get", SERVER_NAME, "--json"])
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| codex_registered_command(&String::from_utf8_lossy(&o.stdout))),
    };
    McpStatus {
        installed: program.is_some(),
        registered: command.is_some(),
        up_to_date: command.as_deref() == Some(expected),
        command,
    }
}

/// 등록한다. 이미 있으면 지우고 다시 등록한다 (앱 위치가 바뀐 경우 등).
pub fn register(client: Client, command: &[String]) -> Result<()> {
    let program = find_program(client.program())
        .with_context(|| format!("`{}` 명령을 찾을 수 없습니다", client.program()))?;
    let _ = unregister(client);
    let mut args: Vec<&str> = match client {
        Client::Claude => vec!["mcp", "add", "--scope", "user", SERVER_NAME, "--"],
        Client::Codex => vec!["mcp", "add", SERVER_NAME, "--"],
    };
    args.extend(command.iter().map(String::as_str));
    let out = run(&program, &args)?;
    if !out.status.success() {
        bail!(
            "등록하지 못했습니다: {}",
            String::from_utf8_lossy(&out.stderr)
                .trim()
                .chars()
                .take(400)
                .collect::<String>()
        );
    }
    Ok(())
}

pub fn unregister(client: Client) -> Result<()> {
    let program = find_program(client.program())
        .with_context(|| format!("`{}` 명령을 찾을 수 없습니다", client.program()))?;
    let args: &[&str] = match client {
        Client::Claude => &["mcp", "remove", "--scope", "user", SERVER_NAME],
        Client::Codex => &["mcp", "remove", SERVER_NAME],
    };
    let out = run(&program, args)?;
    if !out.status.success() {
        bail!(
            "해제하지 못했습니다: {}",
            String::from_utf8_lossy(&out.stderr)
                .trim()
                .chars()
                .take(400)
                .collect::<String>()
        );
    }
    Ok(())
}

/// Antigravity 등 설정 파일을 직접 고치는 MCP 클라이언트에 붙여 넣을 JSON.
pub fn mcp_json_snippet(command: &[String]) -> String {
    let (cmd, args) = command
        .split_first()
        .map_or(("", &[][..]), |(c, a)| (c.as_str(), a));
    serde_json::to_string_pretty(&serde_json::json!({
        "mcpServers": { SERVER_NAME: { "command": cmd, "args": args } }
    }))
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_claude_user_scope_server() {
        let cfg = r#"{"mcpServers":{"exaone":{"type":"stdio","command":"/A/exaone-studio","args":["mcp"]}},"projects":{}}"#;
        assert_eq!(
            claude_registered_command(cfg),
            Some(vec!["/A/exaone-studio".into(), "mcp".into()])
        );
        assert_eq!(claude_registered_command(r#"{"mcpServers":{}}"#), None);
        assert_eq!(claude_registered_command("{}"), None);
    }

    #[test]
    fn reads_codex_get_json() {
        let j = r#"{"name":"exaone","enabled":true,"transport":{"type":"stdio","command":"C:\\X\\exaone-studio.exe","args":["mcp"],"env":null}}"#;
        assert_eq!(
            codex_registered_command(j),
            Some(vec!["C:\\X\\exaone-studio.exe".into(), "mcp".into()])
        );
        assert_eq!(codex_registered_command("not json"), None);
    }

    #[test]
    fn snippet_has_command_and_args() {
        let s = mcp_json_snippet(&["/A/b c".into(), "mcp".into()]);
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["mcpServers"]["exaone"]["command"], "/A/b c");
        assert_eq!(v["mcpServers"]["exaone"]["args"][0], "mcp");
    }
}
