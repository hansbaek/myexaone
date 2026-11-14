//! 게이트웨이 실행 환경. uv로 파이썬 가상 환경을 만들고 LiteLLM을 설치한다.
//!
//! 의존성 목록이 바뀌면(해시 비교) 다시 설치한다. 게이트웨이 설정 파일은 매번 최신으로 덮어쓴다.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

use crate::{assets, launch, paths};

const PYTHON_VERSION: &str = "3.12";
const MARKER: &str = ".requirements.sha256";
/// 마지막으로 게이트웨이용 설정을 쓴 때의 설정 지문
const STAMP: &str = ".config.sha256";

/// 설치된 실행 환경의 파일 위치.
#[derive(Debug, Clone)]
pub struct Runtime {
    pub dir: PathBuf,
    pub venv: PathBuf,
}

impl Runtime {
    pub fn locate() -> Result<Self> {
        Ok(Self {
            dir: paths::runtime_dir()?,
            venv: paths::venv_dir()?,
        })
    }

    pub fn python(&self) -> PathBuf {
        paths::venv_bin(&self.venv, "python")
    }

    pub fn litellm(&self) -> PathBuf {
        paths::venv_bin(&self.venv, "litellm")
    }

    pub fn config_file(&self) -> PathBuf {
        self.dir.join("config.yaml")
    }

    /// 의존성까지 설치가 끝났는지.
    pub fn is_installed(&self) -> bool {
        self.litellm().exists()
            && fs::read_to_string(self.dir.join(MARKER))
                .is_ok_and(|h| h.trim() == requirements_hash())
    }
}

/// 이 프로그램에 들어 있는 게이트웨이 설정(config.yaml + 사용 기록 콜백)의 지문.
pub fn config_stamp() -> String {
    let mut h = Sha256::new();
    h.update(assets::GATEWAY_CONFIG.as_bytes());
    h.update(assets::USAGE_LOGGER.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// 실행 환경에 마지막으로 쓴 설정 지문 (게이트웨이를 마지막으로 띄운 때의 것).
pub fn written_stamp() -> Option<String> {
    let rt = Runtime::locate().ok()?;
    fs::read_to_string(rt.dir.join(STAMP))
        .ok()
        .map(|s| s.trim().to_string())
}

fn requirements_hash() -> String {
    Sha256::digest(assets::REQUIREMENTS.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// uv 실행 파일을 찾는다. `EXAONE_UV` → 이 프로그램과 같은 폴더(앱에 넣은 uv) → PATH 순서.
pub fn find_uv() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("EXAONE_UV")
        .map(PathBuf::from)
        .filter(|p| p.exists())
    {
        return Some(p);
    }
    let name = if cfg!(windows) { "uv.exe" } else { "uv" };
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        let p = dir.join(name);
        if p.exists() {
            return Some(p);
        }
    }
    launch::find_program("uv")
}

/// 실행 환경을 설치하거나 최신으로 맞춘다. `log` 로 진행 상황을 알린다.
pub fn ensure(log: &mut dyn FnMut(&str)) -> Result<Runtime> {
    let rt = Runtime::locate()?;
    fs::create_dir_all(&rt.dir)
        .with_context(|| format!("폴더를 만들 수 없습니다: {}", rt.dir.display()))?;
    fs::write(rt.config_file(), assets::GATEWAY_CONFIG)?;
    fs::write(rt.dir.join("usage_logger.py"), assets::USAGE_LOGGER)?;
    fs::write(rt.dir.join(STAMP), config_stamp())?;

    if rt.is_installed() {
        return Ok(rt);
    }
    let uv = find_uv().context(
        "uv를 찾을 수 없습니다. https://docs.astral.sh/uv/ 에서 설치하거나 EXAONE_UV 로 경로를 지정하세요.",
    )?;

    if !rt.python().exists() {
        log(&format!("파이썬 {PYTHON_VERSION} 가상 환경을 만드는 중…"));
        run(Command::new(&uv)
            .args(["venv", "--python", PYTHON_VERSION])
            .arg(&rt.venv))?;
    }
    log("LiteLLM 설치 중… (처음 한 번, 1~2분)");
    let req = rt.dir.join("requirements.txt");
    fs::write(&req, assets::REQUIREMENTS)?;
    run(Command::new(&uv)
        .args(["pip", "install", "--python"])
        .arg(&rt.venv)
        .arg("-r")
        .arg(&req))?;
    fs::write(rt.dir.join(MARKER), requirements_hash())?;
    log("실행 환경 준비 완료");
    Ok(rt)
}

fn run(cmd: &mut Command) -> Result<()> {
    crate::process::background(cmd);
    let out = cmd
        .output()
        .with_context(|| format!("실행 실패: {cmd:?}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let tail: String = err
            .lines()
            .rev()
            .take(15)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        bail!("명령이 실패했습니다 ({}):\n{tail}", out.status);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_stable_hex() {
        let h = requirements_hash();
        assert_eq!(h.len(), 64);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(h, requirements_hash());
    }

    #[test]
    fn not_installed_in_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let rt = Runtime {
            dir: tmp.path().into(),
            venv: tmp.path().join("venv"),
        };
        assert!(!rt.is_installed());
    }
}
