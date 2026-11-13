//! 데이터 폴더 위치.
//!
//! - macOS: `~/Library/Application Support/EXAONE Studio`
//! - Windows: `%APPDATA%\EXAONE Studio`
//!
//! `EXAONE_STUDIO_HOME` 환경 변수로 바꿀 수 있다 (테스트·CI용).

use std::path::PathBuf;

use anyhow::{Context, Result};

pub const APP_DIR_NAME: &str = "EXAONE Studio";

pub fn data_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("EXAONE_STUDIO_HOME") {
        return Ok(PathBuf::from(dir));
    }
    let base = dirs::data_dir().context("이 OS의 데이터 폴더를 찾을 수 없습니다")?;
    Ok(base.join(APP_DIR_NAME))
}

/// 파이썬 가상 환경과 게이트웨이 설정이 들어가는 폴더.
pub fn runtime_dir() -> Result<PathBuf> {
    Ok(data_dir()?.join("runtime"))
}

pub fn venv_dir() -> Result<PathBuf> {
    Ok(runtime_dir()?.join("venv"))
}

pub fn settings_file() -> Result<PathBuf> {
    Ok(data_dir()?.join("settings.json"))
}

pub fn logs_dir() -> Result<PathBuf> {
    Ok(data_dir()?.join("logs"))
}

/// 가상 환경 안의 실행 파일 경로 (Windows는 `Scripts\이름.exe`).
pub fn venv_bin(venv: &std::path::Path, name: &str) -> PathBuf {
    if cfg!(windows) {
        venv.join("Scripts").join(format!("{name}.exe"))
    } else {
        venv.join("bin").join(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn venv_bin_uses_platform_layout() {
        let p = venv_bin(std::path::Path::new("v"), "python");
        if cfg!(windows) {
            assert!(p.ends_with("Scripts/python.exe") || p.ends_with("Scripts\\python.exe"));
        } else {
            assert!(p.ends_with("bin/python"));
        }
    }
}
