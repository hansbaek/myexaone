//! EXAONE Studio 데스크톱 앱. 핵심 로직은 exaone-core 에 있고, 여기는 화면과 잇는 부분만 둔다.

mod chat;
mod manager;
mod sessions;

use std::sync::Arc;

use exaone_core::settings::Settings;
use exaone_core::{doctor, runtime::Runtime, secrets};
use manager::{LogLine, Manager};
use serde::Serialize;
use sessions::Sessions;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager as _, RunEvent, State, WindowEvent};

type Shared = Arc<Manager>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    /// stopped | starting | running | external | error
    phase: &'static str,
    message: String,
    url: String,
    port: u16,
    /// 가려진 API 키 (없으면 null)
    api_key: Option<String>,
    runtime_installed: bool,
    version: &'static str,
}

#[tauri::command(async)]
fn get_status(mgr: State<'_, Shared>) -> Status {
    let settings = Settings::load().unwrap_or_default();
    let (phase, message) = mgr.refresh(settings.gateway_port);
    Status {
        phase: phase.as_str(),
        message,
        url: settings.gateway_url(),
        port: settings.gateway_port,
        api_key: mgr.key_label(),
        runtime_installed: Runtime::locate().is_ok_and(|rt| rt.is_installed()),
        version: env!("CARGO_PKG_VERSION"),
    }
}

#[tauri::command(async)]
fn start_gateway(mgr: State<'_, Shared>) {
    Manager::start(mgr.inner().clone());
}

#[tauri::command(async)]
fn stop_gateway(mgr: State<'_, Shared>) {
    mgr.stop();
}

#[tauri::command(async)]
fn get_logs(mgr: State<'_, Shared>, after: u64) -> Vec<LogLine> {
    mgr.logs_after(after)
}

#[tauri::command(async)]
fn set_api_key(mgr: State<'_, Shared>, key: String) -> Result<(), String> {
    secrets::set_api_key(&key).map_err(|e| format!("{e:#}"))?;
    mgr.reload_key();
    Ok(())
}

#[tauri::command(async)]
fn delete_api_key(mgr: State<'_, Shared>) -> Result<(), String> {
    secrets::delete_api_key().map_err(|e| format!("{e:#}"))?;
    mgr.reload_key();
    Ok(())
}

#[derive(Serialize)]
struct CheckDto {
    name: &'static str,
    /// ok | warn | fail
    level: &'static str,
    detail: String,
}

#[tauri::command(async)]
fn run_doctor() -> Vec<CheckDto> {
    let settings = Settings::load().unwrap_or_default();
    doctor::run(&settings)
        .into_iter()
        .map(|c| CheckDto {
            name: c.name,
            level: match c.level {
                doctor::Level::Ok => "ok",
                doctor::Level::Warn => "warn",
                doctor::Level::Fail => "fail",
            },
            detail: c.detail,
        })
        .collect()
}

#[tauri::command(async)]
fn open_session(
    app: AppHandle,
    sessions: State<'_, Arc<Sessions>>,
    tool: String,
    cwd: String,
    cols: u16,
    rows: u16,
) -> Result<u32, String> {
    Sessions::open(sessions.inner(), app, &tool, &cwd, cols, rows).map_err(|e| format!("{e:#}"))
}

#[tauri::command(async)]
fn write_session(sessions: State<'_, Arc<Sessions>>, id: u32, data: String) -> Result<(), String> {
    sessions.write(id, &data).map_err(|e| format!("{e:#}"))
}

#[tauri::command(async)]
fn resize_session(
    sessions: State<'_, Arc<Sessions>>,
    id: u32,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    sessions
        .resize(id, cols, rows)
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command(async)]
fn close_session(sessions: State<'_, Arc<Sessions>>, id: u32) {
    sessions.remove(id);
}

fn ime_log_file() -> Result<std::path::PathBuf, String> {
    exaone_core::paths::logs_dir()
        .map(|d| d.join("ime-debug.log"))
        .map_err(|e| e.to_string())
}

/// 한글 입력 진단 기록 (설정에서 켰을 때만 화면이 보낸다).
#[tauri::command(async)]
fn ime_log(line: String) -> Result<(), String> {
    use std::io::Write;
    let path = ime_log_file()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() % 100_000)
        .unwrap_or(0);
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    writeln!(f, "{ms:>5} {line}").map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn ime_log_path() -> Result<String, String> {
    ime_log_file().map(|p| p.display().to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct McpInfo {
    /// 등록할 명령 (이 앱 실행 파일 + "mcp")
    command: Vec<String>,
    /// 설정 파일에 붙여 넣을 JSON (Antigravity 등)
    snippet: String,
    claude: exaone_core::integrations::McpStatus,
    codex: exaone_core::integrations::McpStatus,
}

fn mcp_command() -> Result<Vec<String>, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    Ok(vec![exe.display().to_string(), "mcp".into()])
}

fn client(name: &str) -> Result<exaone_core::integrations::Client, String> {
    match name {
        "claude" => Ok(exaone_core::integrations::Client::Claude),
        "codex" => Ok(exaone_core::integrations::Client::Codex),
        other => Err(format!("알 수 없는 도구: {other}")),
    }
}

#[tauri::command(async)]
fn mcp_info() -> Result<McpInfo, String> {
    use exaone_core::integrations::{self as ig, Client};
    let command = mcp_command()?;
    Ok(McpInfo {
        snippet: ig::mcp_json_snippet(&command),
        claude: ig::status(Client::Claude, &command),
        codex: ig::status(Client::Codex, &command),
        command,
    })
}

#[tauri::command(async)]
fn mcp_register(tool: String) -> Result<(), String> {
    exaone_core::integrations::register(client(&tool)?, &mcp_command()?)
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command(async)]
fn mcp_unregister(tool: String) -> Result<(), String> {
    exaone_core::integrations::unregister(client(&tool)?).map_err(|e| format!("{e:#}"))
}

/// 게이트웨이 사용 기록을 저장소로 옮긴 뒤 `since`(유닉스 초) 이후를 집계한다.
#[tauri::command(async)]
fn usage_summary(since: f64) -> Result<exaone_core::usage::Summary, String> {
    let store = exaone_core::store::Store::open_default().map_err(|e| format!("{e:#}"))?;
    let log = exaone_core::usage::log_file().map_err(|e| format!("{e:#}"))?;
    exaone_core::usage::ingest(&store, &log).map_err(|e| format!("{e:#}"))?;
    exaone_core::usage::summary(&store, since).map_err(|e| format!("{e:#}"))
}

#[tauri::command(async)]
fn home_dir() -> Option<String> {
    std::env::home_dir().map(|p| p.display().to_string())
}

/// 로그인할 때 자동으로 켜지는지
#[tauri::command(async)]
fn autostart_get(app: AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn autostart_set(app: AppHandle, on: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let al = app.autolaunch();
    if on { al.enable() } else { al.disable() }.map_err(|e| e.to_string())
}

/// 로그인 때 자동으로 켜질 때는 창 없이 트레이로만 시작한다.
const HIDDEN_ARG: &str = "--hidden";

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "EXAONE Studio 열기", true, None::<&str>)?;
    let start = MenuItem::with_id(app, "start", "게이트웨이 켜기", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop", "게이트웨이 끄기", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "종료", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &start, &stop, &quit])?;
    let mut tray = TrayIconBuilder::new()
        .tooltip("EXAONE Studio")
        .menu(&menu)
        .on_menu_event(|app, event| {
            let mgr = app.state::<Shared>();
            match event.id.as_ref() {
                "open" => show_main(app),
                "start" => Manager::start(mgr.inner().clone()),
                "stop" => {
                    let mgr = mgr.inner().clone();
                    std::thread::spawn(move || mgr.stop());
                }
                "quit" => app.exit(0),
                _ => {}
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// MCP 서버 모드 (표준 입출력). 종료 코드를 돌려준다.
pub fn run_mcp() -> i32 {
    let settings = Settings::load().unwrap_or_default();
    let key = secrets::api_key().ok().flatten().map(|(k, _)| k);
    match exaone_core::mcp::run_stdio(&settings.api_base, key) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("MCP 서버 오류: {e:#}");
            1
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mgr: Shared = Arc::new(Manager::default());
    let sessions = Arc::new(Sessions::default());
    let sessions_exit = sessions.clone();
    let chat_state = Arc::new(chat::ChatState::default());
    let app = tauri::Builder::default()
        // 이미 떠 있으면 새로 띄우지 않고 기존 창을 앞으로 가져온다 (반드시 첫 플러그인).
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app)
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![HIDDEN_ARG]),
        ))
        .manage(mgr.clone())
        .manage(sessions)
        .manage(chat_state)
        .invoke_handler(tauri::generate_handler![
            get_status,
            start_gateway,
            stop_gateway,
            get_logs,
            set_api_key,
            delete_api_key,
            run_doctor,
            open_session,
            write_session,
            resize_session,
            close_session,
            home_dir,
            ime_log,
            ime_log_path,
            chat::chat_list,
            chat::chat_messages,
            chat::chat_create,
            chat::chat_rename,
            chat::chat_delete,
            chat::chat_stop,
            chat::chat_running,
            chat::chat_send,
            usage_summary,
            mcp_info,
            mcp_register,
            mcp_unregister,
            autostart_get,
            autostart_set
        ])
        .setup(|app| {
            build_tray(app.handle())?;
            // 창은 숨긴 채로 만들어진다 (tauri.conf.json). 자동 실행이 아니면 바로 보여 준다.
            if !std::env::args().any(|a| a == HIDDEN_ARG) {
                show_main(app.handle());
            }
            // 키가 있으면 앱을 켤 때 게이트웨이도 같이 켠다.
            let mgr = app.state::<Shared>().inner().clone();
            std::thread::spawn(move || {
                mgr.reload_key();
                if mgr.key_label().is_some() {
                    Manager::start(mgr);
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            // 창을 닫아도 앱은 트레이에 남아 게이트웨이를 계속 돌린다.
            if let WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(move |app, event| match event {
        RunEvent::Exit => {
            sessions_exit.close_all();
            mgr.stop();
        }
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => show_main(app),
        _ => {
            let _ = app;
        }
    });
}
