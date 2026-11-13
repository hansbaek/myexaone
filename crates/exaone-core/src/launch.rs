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
    fn augmented_path_keeps_existing_and_puts_first_dir_first() {
        let p = augmented_path(Some(Path::new("/x/first")));
        let dirs: Vec<_> = std::env::split_paths(&p).collect();
        assert_eq!(dirs[0], Path::new("/x/first"));
        for d in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
            assert!(dirs.contains(&d));
        }
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

/// EXAONE으로 실행할 수 있는 코딩 도구.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Claude,
    Codex,
}

impl Tool {
    pub fn program_name(self) -> &'static str {
        match self {
            Tool::Claude => "claude",
            Tool::Codex => "codex",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Tool::Claude => "Claude Code",
            Tool::Codex => "Codex",
        }
    }
}

/// 도구를 게이트웨이에 붙여 실행하는 명령을 만든다 (프로그램 찾기와 설정 파일 쓰기 포함).
pub fn tool_command(
    tool: Tool,
    gateway_url: &str,
    gateway_key: &str,
    user_args: &[String],
) -> Result<Command> {
    let program = find_program(tool.program_name()).with_context(|| {
        format!(
            "`{}` 명령을 찾을 수 없습니다. {}를 설치하세요.",
            tool.program_name(),
            tool.label()
        )
    })?;
    let mut cmd = match tool {
        Tool::Claude => {
            let file = write_claude_settings(gateway_url, gateway_key)?;
            claude_command(&program, &file, gateway_url, gateway_key, user_args)
        }
        Tool::Codex => codex_command(&program, gateway_url, gateway_key, user_args),
    };
    cmd.env("PATH", augmented_path(program.parent()));
    Ok(cmd)
}

/// 현재 PATH에 흔한 설치 위치를 더한 값. Finder·시작 메뉴로 띄운 앱은 셸의 PATH를 물려받지 않아,
/// 도구가 내부에서 부르는 `node`, `git` 등을 못 찾는 일을 막는다.
pub fn augmented_path(first: Option<&Path>) -> std::ffi::OsString {
    let mut dirs: Vec<PathBuf> = first.map(Path::to_path_buf).into_iter().collect();
    dirs.extend(
        std::env::var_os("PATH")
            .iter()
            .flat_map(std::env::split_paths),
    );
    for d in extra_dirs() {
        if !dirs.contains(&d) {
            dirs.push(d);
        }
    }
    std::env::join_paths(dirs).unwrap_or_else(|_| std::env::var_os("PATH").unwrap_or_default())
}
