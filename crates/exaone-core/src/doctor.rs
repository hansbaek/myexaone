//! 환경 진단. 연결 문제가 생기면 무엇이 원인인지 한눈에 보이게 한다.

use std::path::PathBuf;

use serde_json::Value;

use crate::launch::find_program;
use crate::runtime::{self, Runtime};
use crate::secrets::{self, KeySource};
use crate::{gateway, settings::Settings};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Clone)]
pub struct Check {
    pub name: &'static str,
    pub level: Level,
    pub detail: String,
}

fn check(name: &'static str, level: Level, detail: impl Into<String>) -> Check {
    Check {
        name,
        level,
        detail: detail.into(),
    }
}

pub fn run(settings: &Settings) -> Vec<Check> {
    let mut out = Vec::new();

    out.push(match runtime::find_uv() {
        Some(p) => check("uv", Level::Ok, p.display().to_string()),
        None => check(
            "uv",
            Level::Fail,
            "찾을 수 없음. https://docs.astral.sh/uv/ 에서 설치하세요.",
        ),
    });

    out.push(match Runtime::locate() {
        Ok(rt) if rt.is_installed() => check("실행 환경", Level::Ok, rt.venv.display().to_string()),
        Ok(_) => check(
            "실행 환경",
            Level::Warn,
            "아직 설치되지 않음. `exaone setup` 을 실행하세요.",
        ),
        Err(e) => check("실행 환경", Level::Fail, e.to_string()),
    });

    out.push(match secrets::api_key() {
        Ok(Some((k, KeySource::Keychain))) => check(
            "API 키",
            Level::Ok,
            format!("키 저장소 · {}", secrets::mask(&k)),
        ),
        Ok(Some((k, KeySource::Env))) => check(
            "API 키",
            Level::Ok,
            format!("환경 변수 EXAONE_API_KEY · {}", secrets::mask(&k)),
        ),
        Ok(None) => check(
            "API 키",
            Level::Fail,
            "없음. `exaone key set` 으로 저장하세요.",
        ),
        Err(e) => check("API 키", Level::Fail, e.to_string()),
    });

    let port = settings.gateway_port;
    let healthy = gateway::is_healthy(port);
    out.push(if healthy {
        check(
            "게이트웨이",
            Level::Ok,
            format!("{} 에서 실행 중", settings.gateway_url()),
        )
    } else if gateway::port_in_use(port) {
        check(
            "게이트웨이",
            Level::Fail,
            format!("{port}번 포트를 다른 프로그램이 쓰고 있습니다"),
        )
    } else {
        check(
            "게이트웨이",
            Level::Warn,
            "꺼져 있음. `exaone gateway` 로 켜세요.",
        )
    });

    // 앱·명령을 새로 깔았는데 게이트웨이가 예전 설정으로 떠 있는 경우
    if healthy && runtime::written_stamp().is_some_and(|s| s != runtime::config_stamp()) {
        out.push(check(
            "게이트웨이 설정",
            Level::Warn,
            "예전 설정으로 실행 중입니다. 게이트웨이를 껐다 켜면 새 설정(사용량 기록 등)이 적용됩니다.",
        ));
    }

    out.push(match crate::usage::log_file() {
        Ok(p) if p.exists() => check("사용량 기록", Level::Ok, p.display().to_string()),
        Ok(_) if healthy => check(
            "사용량 기록",
            Level::Warn,
            "아직 기록이 없습니다. 요청을 보내면 생깁니다. 계속 없으면 게이트웨이를 껐다 켜세요.",
        ),
        Ok(_) => check("사용량 기록", Level::Ok, "게이트웨이를 켜면 기록합니다"),
        Err(e) => check("사용량 기록", Level::Fail, e.to_string()),
    });

    for name in ["claude", "codex"] {
        out.push(match find_program(name) {
            Some(p) => check(
                if name == "claude" {
                    "Claude Code"
                } else {
                    "Codex"
                },
                Level::Ok,
                p.display().to_string(),
            ),
            None => check(
                if name == "claude" {
                    "Claude Code"
                } else {
                    "Codex"
                },
                Level::Warn,
                format!("`{name}` 명령을 찾을 수 없음"),
            ),
        });
    }

    if let Some(path) = claude_user_settings()
        && let Ok(text) = std::fs::read_to_string(&path)
        && let Some(msg) = claude_base_url_conflict(&text, &settings.gateway_url())
    {
        out.push(check(
            "Claude 설정 충돌",
            Level::Warn,
            format!("{}: {msg}", path.display()),
        ));
    }
    out
}

fn claude_user_settings() -> Option<PathBuf> {
    let dir = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".claude")))?;
    Some(dir.join("settings.json"))
}

/// `~/.claude/settings.json` 에 다른 게이트웨이 주소가 있으면 설명을 돌려준다.
///
/// `exaone claude` 는 `--settings` 로 이 값을 이기지만, 평소의 `claude` 는 그 주소로 연결된다.
pub fn claude_base_url_conflict(settings_json: &str, gateway_url: &str) -> Option<String> {
    let v: Value = serde_json::from_str(settings_json).ok()?;
    let url = v.get("env")?.get("ANTHROPIC_BASE_URL")?.as_str()?;
    if url.trim_end_matches('/') == gateway_url.trim_end_matches('/') {
        return None;
    }
    let reachable = url_reachable(url);
    Some(format!(
        "ANTHROPIC_BASE_URL = {url} ({}). `exaone claude` 는 영향이 없지만, 평소의 `claude` 명령은 이 주소로 연결됩니다.",
        if reachable {
            "응답함"
        } else {
            "응답 없음"
        }
    ))
}

fn url_reachable(url: &str) -> bool {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(2)))
        .http_status_as_error(false)
        .build()
        .into();
    agent.get(url).call().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_conflict_when_absent_or_same() {
        assert!(claude_base_url_conflict(r#"{"model":"opus"}"#, "http://127.0.0.1:4000").is_none());
        assert!(
            claude_base_url_conflict(
                r#"{"env":{"ANTHROPIC_BASE_URL":"http://127.0.0.1:4000/"}}"#,
                "http://127.0.0.1:4000"
            )
            .is_none()
        );
        assert!(claude_base_url_conflict("not json", "http://127.0.0.1:4000").is_none());
    }

    #[test]
    fn conflict_when_other_url() {
        let msg = claude_base_url_conflict(
            r#"{"env":{"ANTHROPIC_BASE_URL":"http://127.0.0.1:1/x"}}"#,
            "http://127.0.0.1:4000",
        )
        .unwrap();
        assert!(msg.contains("http://127.0.0.1:1/x"));
        assert!(msg.contains("응답 없음"));
    }
}
