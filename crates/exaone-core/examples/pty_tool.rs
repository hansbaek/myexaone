//! 실제 Claude Code·Codex를 앱과 같은 방식(PTY)으로 실행해 본다. 게이트웨이가 켜져 있어야 한다.
//!
//! cargo run -p exaone-core --example pty_tool -- claude -p "1+1은? 숫자만."
//! cargo run -p exaone-core --example pty_tool -- codex exec --skip-git-repo-check "1+1은? 숫자만."

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use exaone_core::launch::{self, Tool};
use exaone_core::pty::{PtyEvents, PtySession};
use exaone_core::secrets;
use exaone_core::settings::Settings;

struct Print(Mutex<mpsc::Sender<u32>>);

impl PtyEvents for Print {
    fn output(&self, text: String) {
        print!("{text}");
    }
    fn exit(&self, code: u32) {
        let _ = self.0.lock().unwrap().send(code);
    }
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let tool = match args.next().as_deref() {
        Some("claude") => Tool::Claude,
        Some("codex") => Tool::Codex,
        _ => anyhow::bail!("사용: pty_tool <claude|codex> [인자...]"),
    };
    let rest: Vec<String> = args.collect();
    let settings = Settings::load()?;
    let cmd = launch::tool_command(
        tool,
        &settings.gateway_url(),
        &secrets::master_key()?,
        &rest,
    )?;
    let (tx, rx) = mpsc::channel();
    let cwd = std::env::temp_dir();
    let _s = PtySession::spawn(&cmd, &cwd, 100, 30, Arc::new(Print(Mutex::new(tx))))?;
    let code = rx.recv_timeout(Duration::from_secs(300))?;
    println!("\n[exit {code}]");
    Ok(())
}
