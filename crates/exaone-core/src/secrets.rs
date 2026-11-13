//! API 키와 게이트웨이 접속 키. OS 키 저장소(macOS 키체인, Windows 자격 증명 관리자)에 둔다.
//!
//! 환경 변수 `EXAONE_API_KEY`, `LITELLM_MASTER_KEY` 가 있으면 그 값이 우선한다 (CI·테스트용).

use anyhow::{Context, Result};

const SERVICE: &str = "EXAONE Studio";
const API_KEY_ACCOUNT: &str = "exaone-api-key";
const MASTER_KEY_ACCOUNT: &str = "gateway-master-key";

fn entry(account: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, account).context("OS 키 저장소에 접근할 수 없습니다")
}

fn read(account: &str) -> Result<Option<String>> {
    match entry(account)?.get_password() {
        Ok(v) if !v.trim().is_empty() => Ok(Some(v)),
        Ok(_) | Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e).context("OS 키 저장소에서 키를 읽지 못했습니다"),
    }
}

fn env_value(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// 키가 어디서 왔는지 (진단 표시용).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySource {
    Env,
    Keychain,
}

/// EXAONE API 키. 없으면 `Ok(None)`.
pub fn api_key() -> Result<Option<(String, KeySource)>> {
    if let Some(v) = env_value("EXAONE_API_KEY") {
        return Ok(Some((v, KeySource::Env)));
    }
    Ok(read(API_KEY_ACCOUNT)?.map(|v| (v, KeySource::Keychain)))
}

/// EXAONE API 키를 반드시 얻는다. 없으면 안내 메시지와 함께 오류.
pub fn require_api_key() -> Result<String> {
    api_key()?
        .map(|(k, _)| k)
        .context("EXAONE API 키가 없습니다. `exaone key set` 으로 먼저 저장하세요.")
}

pub fn set_api_key(key: &str) -> Result<()> {
    let key = key.trim();
    anyhow::ensure!(!key.is_empty(), "키가 비어 있습니다");
    entry(API_KEY_ACCOUNT)?
        .set_password(key)
        .context("OS 키 저장소에 저장하지 못했습니다")
}

/// 저장된 API 키를 지운다. 원래 없었으면 `false`.
pub fn delete_api_key() -> Result<bool> {
    match entry(API_KEY_ACCOUNT)?.delete_credential() {
        Ok(()) => Ok(true),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(e) => Err(e).context("OS 키 저장소에서 키를 지우지 못했습니다"),
    }
}

/// 게이트웨이 접속 키. 처음 부를 때 임의 값을 만들어 저장한다.
pub fn master_key() -> Result<String> {
    if let Some(v) = env_value("LITELLM_MASTER_KEY") {
        return Ok(v);
    }
    if let Some(v) = read(MASTER_KEY_ACCOUNT)? {
        return Ok(v);
    }
    let key = new_master_key();
    entry(MASTER_KEY_ACCOUNT)?
        .set_password(&key)
        .context("게이트웨이 키를 저장하지 못했습니다")?;
    Ok(key)
}

fn new_master_key() -> String {
    format!("sk-local-{}", uuid::Uuid::new_v4().simple())
}

/// 화면에 보여 줄 때 앞 4자만 남긴다.
pub fn mask(key: &str) -> String {
    let head: String = key.chars().take(4).collect();
    format!("{head}… ({}자)", key.chars().count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn master_key_format() {
        let k = new_master_key();
        assert!(k.starts_with("sk-local-"));
        assert_eq!(k.len(), "sk-local-".len() + 32);
        assert_ne!(k, new_master_key());
    }

    #[test]
    fn mask_hides_rest() {
        assert_eq!(mask("sk-abcdef"), "sk-a… (9자)");
        assert_eq!(mask("ab"), "ab… (2자)");
    }
}
