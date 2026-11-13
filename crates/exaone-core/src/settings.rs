//! 사용자 설정 (데이터 폴더의 settings.json). 환경 변수가 있으면 그 값이 우선한다.

use std::fs;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::paths;

pub const DEFAULT_PORT: u16 = 4000;
pub const DEFAULT_API_BASE: &str = "https://api.exaone.ai/v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// 게이트웨이가 열 로컬 포트 (`GATEWAY_PORT`).
    pub gateway_port: u16,
    /// EXAONE API 주소 (`EXAONE_API_BASE`). 가짜 서버로 시험할 때만 바꾼다.
    pub api_base: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            gateway_port: DEFAULT_PORT,
            api_base: DEFAULT_API_BASE.to_string(),
        }
    }
}

impl Settings {
    /// 파일 값을 읽고 환경 변수로 덮어쓴다. 파일이 없으면 기본값.
    pub fn load() -> Result<Self> {
        let path = paths::settings_file()?;
        let mut s = match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text)
                .with_context(|| format!("설정 파일을 읽을 수 없습니다: {}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Settings::default(),
            Err(e) => return Err(e).context("설정 파일 읽기 실패"),
        };
        s.apply_env(|k| std::env::var(k).ok());
        Ok(s)
    }

    pub fn save(&self) -> Result<()> {
        let path = paths::settings_file()?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(&path, serde_json::to_string_pretty(self)? + "\n")?;
        Ok(())
    }

    fn apply_env(&mut self, get: impl Fn(&str) -> Option<String>) {
        if let Some(port) = get("GATEWAY_PORT").and_then(|v| v.trim().parse().ok()) {
            self.gateway_port = port;
        }
        if let Some(base) = get("EXAONE_API_BASE").filter(|v| !v.trim().is_empty()) {
            self.api_base = base.trim().to_string();
        }
    }

    pub fn gateway_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.gateway_port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_overrides_file_values() {
        let mut s = Settings::default();
        s.apply_env(|k| match k {
            "GATEWAY_PORT" => Some("4100".into()),
            "EXAONE_API_BASE" => Some(" http://127.0.0.1:9/v1 ".into()),
            _ => None,
        });
        assert_eq!(s.gateway_port, 4100);
        assert_eq!(s.api_base, "http://127.0.0.1:9/v1");
        assert_eq!(s.gateway_url(), "http://127.0.0.1:4100");
    }

    #[test]
    fn bad_env_values_are_ignored() {
        let mut s = Settings::default();
        s.apply_env(|k| match k {
            "GATEWAY_PORT" => Some("abc".into()),
            "EXAONE_API_BASE" => Some("  ".into()),
            _ => None,
        });
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn partial_file_uses_defaults() {
        let s: Settings = serde_json::from_str(r#"{"gateway_port": 4200}"#).unwrap();
        assert_eq!(s.gateway_port, 4200);
        assert_eq!(s.api_base, DEFAULT_API_BASE);
    }
}
