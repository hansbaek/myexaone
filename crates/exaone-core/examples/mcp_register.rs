//! MCP 등록·상태·해제를 실제 claude/codex 명령으로 해 본다.
//! 사용자 설정을 건드리지 않으려면 CLAUDE_CONFIG_DIR, CODEX_HOME 을 임시 폴더로 지정해서 실행한다.
//!
//! cargo run -p exaone-core --example mcp_register -- <MCP 서버 실행 파일> [등록 후 남겨 둘지: keep]

use exaone_core::integrations::{self, Client};

fn main() -> anyhow::Result<()> {
    let exe = std::env::args().nth(1).expect("MCP 서버 실행 파일 경로");
    let keep = std::env::args().nth(2).as_deref() == Some("keep");
    let cmd = vec![exe, "mcp".to_string()];
    for c in [Client::Claude, Client::Codex] {
        println!("{c:?} 등록 전: {:?}", integrations::status(c, &cmd));
        integrations::register(c, &cmd)?;
        println!("{c:?} 등록 후: {:?}", integrations::status(c, &cmd));
        integrations::register(c, &cmd)?; // 다시 등록해도 문제없다
        if !keep {
            integrations::unregister(c)?;
            println!("{c:?} 해제 후: {:?}", integrations::status(c, &cmd));
        }
    }
    Ok(())
}
