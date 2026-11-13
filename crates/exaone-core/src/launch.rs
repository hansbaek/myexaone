//! Claude Code·Codex를 게이트웨이에 붙여 실행하는 명령을 만든다.
//!
//! 두 도구의 평소 설정과 로그인은 바꾸지 않는다. 이 실행에만 적용되는 값을 넘긴다.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use serde_json::json;

use crate::{GATEWAY_MODEL, paths};

/// K-EXAONE 2.5 컨텍스트(524,288 토큰)에서 출력 여유분을 뺀 값.
const CONTEXT_TOKENS: &str = "480000";
/// Codex가 게이트웨이 키를 읽을 환경 변수 이름.
const CODEX_KEY_ENV: &str = "EXAONE_GATEWAY_KEY";

/// 프로그램을 찾는다. PATH에 없으면 흔한 설치 위치도 본다
/// (Finder·시작 메뉴로 띄운 앱은 셸의 PATH를 물려받지 않기 때문).
pub fn find_program(name: &str) -> Option<PathBuf> {
    if let Ok(p) = which::which(name) {
        return Some(p);
    }
    let exts: &[&str] = if cfg!(windows) {
        &[".exe", ".cmd", ".bat"]
    } else {
        &[""]
    };
    extra_dirs()
        .into_iter()
        .flat_map(|d| exts.iter().map(move |e| d.join(format!("{name}{e}"))))
        .find(|p| p.is_file())
}

fn extra_dirs() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut v = vec![
        home.join(".local/bin"),
        home.join(".cargo/bin"),
        home.join(".bun/bin"),
        home.join(".npm-global/bin"),
    ];
    if cfg!(windows) {
        if let Some(appdata) = dirs::data_dir() {
            v.push(appdata.join("npm"));
        }
        if let Some(local) = dirs::data_local_dir() {
            v.push(local.join("Programs").join("claude"));
        }
    } else {
        v.extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"].map(PathBuf::from));
    }
    v
}

/// Claude Code에 `--settings` 로 넘길 설정.
///
/// Claude Code는 셸 환경 변수보다 `~/.claude/settings.json` 의 env 값을 우선하므로,
/// 그 파일에 다른 `ANTHROPIC_BASE_URL` 이 있어도 이기도록 `--settings` 로 넘긴다
/// (`--settings` 는 사용자 설정 파일보다 우선한다).
pub fn claude_settings(gateway_url: &str, gateway_key: &str) -> serde_json::Value {
    let m = GATEWAY_MODEL;
    json!({ "env": {
        "ANTHROPIC_BASE_URL": gateway_url,
        "ANTHROPIC_AUTH_TOKEN": gateway_key,
        "ANTHROPIC_MODEL": m,
        "ANTHROPIC_DEFAULT_OPUS_MODEL": m,
        "ANTHROPIC_DEFAULT_SONNET_MODEL": m,
        "ANTHROPIC_DEFAULT_HAIKU_MODEL": m,
        "ANTHROPIC_SMALL_FAST_MODEL": m,
        "CLAUDE_CODE_MAX_CONTEXT_TOKENS": CONTEXT_TOKENS,
        // 한도가 낮으므로 필수 아닌 호출은 끈다.
        "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1",
    }})
}

/// 설정을 데이터 폴더의 파일로 써 두고 그 경로를 돌려준다.
/// JSON을 명령줄에 직접 넣으면 Windows의 `claude.cmd` 를 거칠 때 따옴표가 깨질 수 있어 파일로 넘긴다.
pub fn write_claude_settings(gateway_url: &str, gateway_key: &str) -> Result<PathBuf> {
    let dir = paths::data_dir()?;
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("claude-settings.json");
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&claude_settings(gateway_url, gateway_key))?,
    )
    .context("Claude 설정 파일을 쓰지 못했습니다")?;
    restrict_permissions(&path);
    Ok(path)
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) {}

pub fn claude_command(
    program: &Path,
    settings_file: &Path,
    gateway_url: &str,
    gateway_key: &str,
    user_args: &[String],
) -> Command {
    let mut cmd = Command::new(program);
    cmd.arg("--settings")
        .arg(settings_file)
        .args(["--model", GATEWAY_MODEL])
        .args(user_args);
    cmd.env_remove("ANTHROPIC_API_KEY").env_remove("CLAUDECODE");
    // 설정 파일을 읽지 않는 경로를 위해 환경 변수도 같이 둔다.
    cmd.env("ANTHROPIC_BASE_URL", gateway_url)
        .env("ANTHROPIC_AUTH_TOKEN", gateway_key);
    cmd
}

/// Codex `-c` 덮어쓰기 인자. 따옴표가 필요 없는 값만 써서 OS마다 인자 처리가 달라도 안전하게 한다.
pub fn codex_args(gateway_url: &str) -> Vec<String> {
    [
        "model_providers.exaone.name=EXAONE".to_string(),
        format!("model_providers.exaone.base_url={gateway_url}/v1"),
        format!("model_providers.exaone.env_key={CODEX_KEY_ENV}"),
        "model_providers.exaone.wire_api=responses".to_string(),
        "model_provider=exaone".to_string(),
        format!("model={GATEWAY_MODEL}"),
        "model_reasoning_effort=high".to_string(),
    ]
    .into_iter()
    .flat_map(|kv| ["-c".to_string(), kv])
    .collect()
}

pub fn codex_command(
    program: &Path,
    gateway_url: &str,
    gateway_key: &str,
    user_args: &[String],
) -> Command {
    let mut cmd = Command::new(program);
    cmd.args(codex_args(gateway_url))
        .args(user_args)
        .env(CODEX_KEY_ENV, gateway_key);
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_settings_point_everything_to_gateway() {
        let s = claude_settings("http://127.0.0.1:4000", "sk-x");
        let env = s["env"].as_object().unwrap();
        assert_eq!(env["ANTHROPIC_BASE_URL"], "http://127.0.0.1:4000");
        assert_eq!(env["ANTHROPIC_AUTH_TOKEN"], "sk-x");
        for k in [
            "ANTHROPIC_MODEL",
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "ANTHROPIC_SMALL_FAST_MODEL",
        ] {
            assert_eq!(env[k], GATEWAY_MODEL, "{k}");
        }
    }

    #[test]
    fn claude_command_removes_conflicting_env() {
        let cmd = claude_command(
            Path::new("claude"),
            Path::new("s.json"),
            "http://g",
            "k",
            &["-p".into(), "hi".into()],
        );
        let args: Vec<_> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            ["--settings", "s.json", "--model", GATEWAY_MODEL, "-p", "hi"]
        );
        let removed: Vec<_> = cmd
            .get_envs()
            .filter(|(_, v)| v.is_none())
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect();
        assert!(removed.contains(&"ANTHROPIC_API_KEY".to_string()));
    }

    #[test]
    fn codex_args_have_no_quotes() {
        let a = codex_args("http://127.0.0.1:4000");
        assert!(a.iter().all(|x| !x.contains('"')));
        assert!(
            a.contains(&"model_providers.exaone.base_url=http://127.0.0.1:4000/v1".to_string())
        );
        assert!(a.contains(&"model_providers.exaone.wire_api=responses".to_string()));
        assert_eq!(a.iter().filter(|x| *x == "-c").count(), a.len() / 2);
    }
}
