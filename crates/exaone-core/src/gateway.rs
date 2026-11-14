//! LiteLLM 게이트웨이 프로세스.

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::runtime::Runtime;
use crate::settings::Settings;

/// 게이트웨이를 띄우는 데 필요한 값.
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub port: u16,
    pub api_base: String,
    pub api_key: String,
    pub master_key: String,
    /// 요청마다 사용 기록(JSON 한 줄)을 남길 파일
    pub usage_log: Option<std::path::PathBuf>,
}

impl GatewayConfig {
    pub fn new(settings: &Settings, api_key: String, master_key: String) -> Self {
        let usage_log = crate::usage::log_file().ok();
        if let Some(dir) = usage_log.as_ref().and_then(|p| p.parent()) {
            let _ = std::fs::create_dir_all(dir);
        }
        Self {
            port: settings.gateway_port,
            api_base: settings.api_base.clone(),
            api_key,
            master_key,
            usage_log,
        }
    }
}

/// 게이트웨이 프로세스 명령을 만든다. 출력 연결은 호출하는 쪽이 정한다.
pub fn command(rt: &Runtime, cfg: &GatewayConfig) -> Command {
    let mut cmd = Command::new(rt.litellm());
    cmd.arg("--config")
        .arg(rt.config_file())
        .args(["--host", "127.0.0.1", "--port", &cfg.port.to_string()])
        .env("EXAONE_API_KEY", &cfg.api_key)
        .env("EXAONE_API_BASE", &cfg.api_base)
        .env("LITELLM_MASTER_KEY", &cfg.master_key)
        // 설정 파일의 한국어 주석 때문에 Windows(cp949)에서도 UTF-8로 읽게 한다.
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8")
        // 사용 통계 등 외부 전송을 끈다.
        .env("LITELLM_TELEMETRY", "False");
    if let Some(p) = &cfg.usage_log {
        cmd.env("EXAONE_USAGE_LOG", p);
    }
    cmd
}

/// 앱에서 쓰는 실행 방식: 콘솔 창 없이 띄우고 출력은 파이프로 받는다. 끌 때는 [`crate::process::kill_tree`].
pub fn spawn_background(rt: &Runtime, cfg: &GatewayConfig) -> Result<Child> {
    let mut cmd = command(rt, cfg);
    crate::process::background(&mut cmd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("게이트웨이를 실행하지 못했습니다")
}

pub fn spawn(rt: &Runtime, cfg: &GatewayConfig, stdout: Stdio, stderr: Stdio) -> Result<Child> {
    command(rt, cfg)
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .spawn()
        .context("게이트웨이를 실행하지 못했습니다")
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .build()
        .into()
}

/// 게이트웨이가 응답하는지.
pub fn is_healthy(port: u16) -> bool {
    agent(Duration::from_secs(2))
        .get(&format!("http://127.0.0.1:{port}/health/liveliness"))
        .call()
        .is_ok_and(|r| r.status().is_success())
}

/// 게이트웨이가 준비될 때까지 기다린다. 프로세스가 먼저 끝나면 오류.
pub fn wait_healthy(port: u16, child: &mut Child, timeout: Duration) -> Result<()> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Some(status) = child.try_wait()? {
            bail!("게이트웨이가 시작 중에 종료됐습니다 ({status})");
        }
        if is_healthy(port) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    bail!(
        "게이트웨이가 {}초 안에 준비되지 않았습니다",
        timeout.as_secs()
    )
}

/// 게이트웨이가 준비될 때까지 기다린다. `exited` 가 참이 되면(프로세스 종료) 바로 오류.
pub fn wait_healthy_with(
    port: u16,
    timeout: Duration,
    mut exited: impl FnMut() -> bool,
) -> Result<()> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if exited() {
            bail!("게이트웨이가 시작 중에 종료됐습니다");
        }
        if is_healthy(port) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    bail!(
        "게이트웨이가 {}초 안에 준비되지 않았습니다",
        timeout.as_secs()
    )
}

/// 포트를 이미 다른 프로그램이 쓰고 있는지.
pub fn port_in_use(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_err()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_sets_env_and_args() {
        let rt = Runtime {
            dir: "rt".into(),
            venv: "rt/venv".into(),
        };
        let cfg = GatewayConfig {
            port: 4321,
            api_base: "http://x/v1".into(),
            api_key: "k".into(),
            master_key: "m".into(),
            usage_log: Some("u.jsonl".into()),
        };
        let cmd = command(&rt, &cfg);
        let args: Vec<_> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(args.windows(2).any(|w| w == ["--port", "4321"]));
        assert!(args.windows(2).any(|w| w == ["--host", "127.0.0.1"]));
        let env: Vec<_> = cmd
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect();
        assert!(env.contains(&("PYTHONUTF8".into(), Some("1".into()))));
        assert!(env.contains(&("EXAONE_USAGE_LOG".into(), Some("u.jsonl".into()))));
        assert!(env.contains(&("EXAONE_API_BASE".into(), Some("http://x/v1".into()))));
    }

    #[test]
    fn bound_port_is_in_use() {
        let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = l.local_addr().unwrap().port();
        assert!(port_in_use(port));
        // 닫은 뒤의 확인은 하지 않는다: 병렬 테스트의 바깥 연결이 같은 임시 포트를 가져갈 수 있다.
        drop(l);
    }
}
