// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `EXAONE Studio mcp`: 화면 없이 exaone_chat MCP 서버로 동작한다 (Claude Code·Codex 등이 실행).
    if std::env::args().nth(1).as_deref() == Some("mcp") {
        std::process::exit(exaone_studio_lib::run_mcp());
    }
    exaone_studio_lib::run()
}
