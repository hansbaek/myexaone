//! 앱이 띄운 게이트웨이 프로세스의 상태와 로그.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::process::Child;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use exaone_core::gateway::{self, GatewayConfig};
use exaone_core::settings::Settings;
use exaone_core::{process, runtime, secrets};
use serde::Serialize;

const MAX_LOG_LINES: usize = 2000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    #[default]
    Stopped,
    Starting,
    Running,
    /// 앱이 띄우지 않은 게이트웨이(예: 터미널의 `exaone gateway`)가 이미 돌고 있음
    External,
    Error,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Stopped => "stopped",
            Phase::Starting => "starting",
            Phase::Running => "running",
            Phase::External => "external",
            Phase::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LogLine {
    pub seq: u64,
    pub text: String,
}

#[derive(Default)]
struct Inner {
    child: Option<Child>,
    phase: Phase,
    message: String,
    logs: VecDeque<LogLine>,
    seq: u64,
    key_label: Option<String>,
}

#[derive(Default)]
pub struct Manager {
    inner: Mutex<Inner>,
}

impl Manager {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn log(&self, text: impl Into<String>) {
        let mut g = self.lock();
        g.seq += 1;
        let seq = g.seq;
        g.logs.push_back(LogLine {
            seq,
            text: text.into(),
        });
        while g.logs.len() > MAX_LOG_LINES {
            g.logs.pop_front();
        }
    }

    pub fn logs_after(&self, after: u64) -> Vec<LogLine> {
        self.lock()
            .logs
            .iter()
            .filter(|l| l.seq > after)
            .cloned()
            .collect()
    }

    fn set(&self, phase: Phase, message: impl Into<String>) {
        let mut g = self.lock();
        g.phase = phase;
        g.message = message.into();
    }

    /// OS 키 저장소에서 키를 다시 읽는다 (처음 읽을 때 OS가 접근 허용을 물을 수 있다).
    pub fn reload_key(&self) {
        let label = match secrets::api_key() {
            Ok(Some((k, _))) => Some(secrets::mask(&k)),
            Ok(None) => None,
            Err(e) => {
                self.log(format!("키 저장소 오류: {e:#}"));
                None
            }
        };
        self.lock().key_label = label;
    }

    pub fn key_label(&self) -> Option<String> {
        self.lock().key_label.clone()
    }

    /// 실제 프로세스 상태를 반영해 현재 상태를 돌려준다.
    pub fn refresh(&self, port: u16) -> (Phase, String) {
        let phase = {
            let mut g = self.lock();
            if g.phase == Phase::Running
                && let Some(child) = g.child.as_mut()
                && let Ok(Some(status)) = child.try_wait()
            {
                g.child = None;
                g.phase = Phase::Error;
                g.message = format!("게이트웨이가 예기치 않게 종료됐습니다 ({status})");
            }
            g.phase
        };
        // 바깥 게이트웨이는 우리가 관리하지 않으므로 살아 있는지만 본다.
        match phase {
            Phase::External if !gateway::is_healthy(port) => self.set(Phase::Stopped, ""),
            Phase::Stopped if gateway::is_healthy(port) => {
                self.set(Phase::External, "다른 곳에서 띄운 게이트웨이를 사용 중")
            }
            _ => {}
        }
        let g = self.lock();
        (g.phase, g.message.clone())
    }

    /// 게이트웨이를 켠다. 설치와 시작은 시간이 걸리므로 별도 스레드에서 진행한다.
    pub fn start(this: Arc<Self>) {
        {
            let mut g = this.lock();
            if matches!(g.phase, Phase::Starting | Phase::Running) {
                return;
            }
            g.phase = Phase::Starting;
            g.message = "시작하는 중…".into();
        }
        std::thread::spawn(move || {
            if let Err(e) = Self::start_blocking(&this) {
                let msg = format!("{e:#}");
                this.log(format!("오류: {msg}"));
                this.set(Phase::Error, msg);
            }
        });
    }

    fn start_blocking(this: &Arc<Self>) -> anyhow::Result<()> {
        let settings = Settings::load()?;
        let port = settings.gateway_port;
        if gateway::is_healthy(port) {
            this.set(Phase::External, "다른 곳에서 띄운 게이트웨이를 사용 중");
            return Ok(());
        }
        if gateway::port_in_use(port) {
            anyhow::bail!("{port}번 포트를 다른 프로그램이 쓰고 있습니다");
        }
        let api_key = secrets::require_api_key()
            .map_err(|_| anyhow::anyhow!("API 키가 없습니다. 설정에서 키를 입력하세요."))?;
        let master_key = secrets::master_key()?;

        this.set(Phase::Starting, "실행 환경 확인 중…");
        let rt = runtime::ensure(&mut |m| {
            this.log(m);
            this.set(Phase::Starting, m);
        })?;

        this.set(Phase::Starting, "게이트웨이 시작 중…");
        let cfg = GatewayConfig::new(&settings, api_key, master_key);
        let mut child = gateway::spawn_background(&rt, &cfg)?;
        if let Some(out) = child.stdout.take() {
            Self::pipe_logs(this.clone(), out);
        }
        if let Some(err) = child.stderr.take() {
            Self::pipe_logs(this.clone(), err);
        }
        this.lock().child = Some(child);

        let exited = || {
            let mut g = this.lock();
            // 시작 중에 사용자가 끄면 child 가 없어진다.
            g.child
                .as_mut()
                .is_none_or(|c| matches!(c.try_wait(), Ok(Some(_))))
        };
        match gateway::wait_healthy_with(port, Duration::from_secs(180), exited) {
            Ok(()) => {
                this.log(format!("게이트웨이 준비 완료: {}", settings.gateway_url()));
                this.set(Phase::Running, "");
                Ok(())
            }
            Err(e) => {
                let stopped_by_user = this.lock().phase == Phase::Stopped;
                this.kill_child();
                if stopped_by_user { Ok(()) } else { Err(e) }
            }
        }
    }

    fn pipe_logs(this: Arc<Self>, stream: impl Read + Send + 'static) {
        std::thread::spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                if !line.trim().is_empty() {
                    this.log(line);
                }
            }
        });
    }

    fn kill_child(&self) {
        let child = self.lock().child.take();
        if let Some(mut child) = child {
            process::kill_tree(&mut child, Duration::from_secs(5));
        }
    }

    /// 앱이 띄운 게이트웨이를 끈다. 바깥에서 띄운 것은 건드리지 않는다.
    pub fn stop(&self) {
        let had_child = self.lock().child.is_some();
        self.set(Phase::Stopped, "");
        self.kill_child();
        if had_child {
            self.log("게이트웨이를 껐습니다");
        }
    }
}
