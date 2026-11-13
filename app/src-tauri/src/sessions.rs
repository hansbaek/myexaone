//! 앱 안 터미널 세션 (Claude Code·Codex). PTY 처리는 exaone-core 에 있다.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use exaone_core::launch::{self, Tool};
use exaone_core::pty::{PtyEvents, PtySession};
use exaone_core::settings::Settings;
use exaone_core::{gateway, secrets};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

#[derive(Default)]
pub struct Sessions {
    next_id: AtomicU32,
    map: Mutex<HashMap<u32, PtySession>>,
}

#[derive(Clone, Serialize)]
struct OutputEvent {
    id: u32,
    data: String,
}

#[derive(Clone, Serialize)]
struct ExitEvent {
    id: u32,
    code: u32,
}

struct Forward {
    id: u32,
    app: AppHandle,
    sessions: Arc<Sessions>,
}

impl PtyEvents for Forward {
    fn output(&self, text: String) {
        let _ = self.app.emit(
            "session-output",
            OutputEvent {
                id: self.id,
                data: text,
            },
        );
    }

    fn exit(&self, code: u32) {
        self.sessions.remove(self.id);
        let _ = self
            .app
            .emit("session-exit", ExitEvent { id: self.id, code });
    }
}

impl Sessions {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<u32, PtySession>> {
        self.map.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn open(
        this: &Arc<Self>,
        app: AppHandle,
        tool: &str,
        cwd: &str,
        cols: u16,
        rows: u16,
    ) -> anyhow::Result<u32> {
        let tool = match tool {
            "claude" => Tool::Claude,
            "codex" => Tool::Codex,
            other => anyhow::bail!("알 수 없는 도구: {other}"),
        };
        let cwd = PathBuf::from(cwd);
        anyhow::ensure!(cwd.is_dir(), "폴더가 없습니다: {}", cwd.display());
        let settings = Settings::load()?;
        anyhow::ensure!(
            gateway::is_healthy(settings.gateway_port),
            "게이트웨이가 꺼져 있습니다. 대시보드에서 먼저 켜세요."
        );
        let cmd =
            launch::tool_command(tool, &settings.gateway_url(), &secrets::master_key()?, &[])?;
        let id = this.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let events = Arc::new(Forward {
            id,
            app,
            sessions: this.clone(),
        });
        let session = PtySession::spawn(&cmd, &cwd, cols, rows, events)?;
        this.lock().insert(id, session);
        Ok(id)
    }

    pub fn write(&self, id: u32, data: &str) -> anyhow::Result<()> {
        match self.lock().get_mut(&id) {
            Some(s) => s.write(data),
            None => Ok(()),
        }
    }

    pub fn resize(&self, id: u32, cols: u16, rows: u16) -> anyhow::Result<()> {
        match self.lock().get(&id) {
            Some(s) => s.resize(cols, rows),
            None => Ok(()),
        }
    }

    /// 세션을 닫는다 (프로그램 종료).
    pub fn remove(&self, id: u32) {
        let session = self.lock().remove(&id);
        drop(session);
    }

    pub fn close_all(&self) {
        let all: Vec<PtySession> = self.lock().drain().map(|(_, s)| s).collect();
        drop(all);
    }
}
