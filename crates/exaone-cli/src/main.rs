//! `exaone` 명령: EXAONE 게이트웨이를 켜고, Claude Code·Codex를 EXAONE으로 실행한다.
//! macOS·Windows 공용 (기존 bin/*.sh 를 대신한다).

use std::io::{BufRead, IsTerminal};
use std::process::{Command, ExitCode, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use exaone_core::doctor::{self, Level};
use exaone_core::gateway::{self, GatewayConfig};
use exaone_core::settings::Settings;
use exaone_core::{assets, launch, mcp, paths, runtime, secrets};

#[derive(Parser)]
#[command(
    name = "exaone",
    version,
    about = "EXAONE 게이트웨이와 Claude Code·Codex 실행 도구"
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 실행 환경(LiteLLM)을 설치하고, API 키가 없으면 입력받는다
    Setup {
        /// API 키를 묻지 않는다
        #[arg(long)]
        no_key: bool,
    },
    /// API 키 관리 (OS 키 저장소)
    Key {
        #[command(subcommand)]
        action: KeyCmd,
    },
    /// 게이트웨이를 이 터미널에서 실행한다 (Ctrl+C로 종료)
    Gateway {
        /// 포트 (기본 4000)
        #[arg(long)]
        port: Option<u16>,
    },
    /// Claude Code를 EXAONE으로 실행한다. 뒤의 인자는 claude 에 그대로 전달된다
    #[command(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        disable_help_flag = true
    )]
    Claude { args: Vec<String> },
    /// Codex CLI를 EXAONE으로 실행한다. 뒤의 인자는 codex 에 그대로 전달된다
    #[command(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        disable_help_flag = true
    )]
    Codex { args: Vec<String> },
    /// EXAONE을 exaone_chat 도구로 노출하는 MCP 서버 (stdio)
    Mcp,
    /// 실제 EXAONE API의 도구 호출·구조화 출력·한도를 점검한다
    Verify,
    /// 설치 상태, 키, 게이트웨이, 설정 충돌을 진단한다
    Doctor,
}

#[derive(Subcommand)]
enum KeyCmd {
    /// API 키를 저장한다 (화면에 표시되지 않음)
    Set {
        /// 표준 입력에서 한 줄로 읽는다 (스크립트용)
        #[arg(long)]
        stdin: bool,
    },
    /// 저장 여부를 확인한다
    Status,
    /// 저장된 키를 지운다
    Delete,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli.command) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("오류: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cmd: Cmd) -> Result<ExitCode> {
    match cmd {
        Cmd::Setup { no_key } => setup(no_key),
        Cmd::Key { action } => key(action),
        Cmd::Gateway { port } => run_gateway(port),
        Cmd::Claude { args } => run_tool(Tool::Claude, &args),
        Cmd::Codex { args } => run_tool(Tool::Codex, &args),
        Cmd::Mcp => {
            let settings = Settings::load()?;
            let key = secrets::api_key().ok().flatten().map(|(k, _)| k);
            mcp::run_stdio(&settings.api_base, key)?;
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Verify => verify(),
        Cmd::Doctor => doctor(),
    }
}

fn progress(msg: &str) {
    eprintln!("· {msg}");
}

fn setup(no_key: bool) -> Result<ExitCode> {
    runtime::ensure(&mut |m| progress(m))?;
    if !no_key && secrets::api_key()?.is_none() {
        set_key(false)?;
    }
    eprintln!("준비 완료. `exaone gateway` 로 게이트웨이를 켜세요.");
    Ok(ExitCode::SUCCESS)
}

fn key(action: KeyCmd) -> Result<ExitCode> {
    match action {
        KeyCmd::Set { stdin } => set_key(stdin)?,
        KeyCmd::Status => match secrets::api_key()? {
            Some((k, src)) => println!("저장됨 ({:?}) · {}", src, secrets::mask(&k)),
            None => println!("저장된 키 없음"),
        },
        KeyCmd::Delete => {
            if secrets::delete_api_key()? {
                println!("키를 지웠습니다");
            } else {
                println!("저장된 키가 없습니다");
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn set_key(from_stdin: bool) -> Result<()> {
    let key = if from_stdin || !std::io::stdin().is_terminal() {
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line)?;
        line
    } else {
        rpassword::prompt_password("EXAONE API 키를 붙여넣고 Enter (화면에 표시되지 않음): ")?
    };
    secrets::set_api_key(&key)?;
    eprintln!(
        "API 키를 OS 키 저장소에 저장했습니다 ({}).",
        secrets::mask(key.trim())
    );
    Ok(())
}

/// 자식 프로세스가 도는 동안 Ctrl+C는 자식이 처리하게 하고, 이 프로세스는 무시한다.
fn ignore_ctrl_c() {
    let _ = ctrlc::set_handler(|| {});
}

fn run_gateway(port: Option<u16>) -> Result<ExitCode> {
    let mut settings = Settings::load()?;
    if let Some(p) = port {
        settings.gateway_port = p;
    }
    let port = settings.gateway_port;
    if gateway::is_healthy(port) {
        eprintln!(
            "게이트웨이가 이미 {} 에서 실행 중입니다.",
            settings.gateway_url()
        );
        return Ok(ExitCode::SUCCESS);
    }
    if gateway::port_in_use(port) {
        bail!(
            "{port}번 포트를 다른 프로그램이 쓰고 있습니다. `exaone gateway --port <번호>` 로 바꾸세요."
        );
    }
    let rt = runtime::ensure(&mut |m| progress(m))?;
    let cfg = GatewayConfig::new(
        &settings,
        secrets::require_api_key()?,
        secrets::master_key()?,
    );

    ignore_ctrl_c();
    let mut child = gateway::spawn(&rt, &cfg, Stdio::inherit(), Stdio::inherit())?;
    match gateway::wait_healthy(port, &mut child, Duration::from_secs(180)) {
        Ok(()) => eprintln!(
            "\n게이트웨이 준비 완료: {} (Ctrl+C로 종료)\n",
            settings.gateway_url()
        ),
        Err(e) => {
            let _ = child.kill();
            return Err(e);
        }
    }
    let status = child.wait()?;
    Ok(exit_code(status.code()))
}

#[derive(Clone, Copy)]
enum Tool {
    Claude,
    Codex,
}

fn run_tool(tool: Tool, args: &[String]) -> Result<ExitCode> {
    let settings = Settings::load()?;
    if !gateway::is_healthy(settings.gateway_port) {
        bail!(
            "게이트웨이가 꺼져 있습니다 ({}). 다른 터미널에서 `exaone gateway` 를 먼저 실행하세요.",
            settings.gateway_url()
        );
    }
    let url = settings.gateway_url();
    let key = secrets::master_key()?;
    let cmd = match tool {
        Tool::Claude => {
            let program = launch::find_program("claude")
                .context("`claude` 명령을 찾을 수 없습니다. Claude Code를 설치하세요.")?;
            let file = launch::write_claude_settings(&url, &key)?;
            launch::claude_command(&program, &file, &url, &key, args)
        }
        Tool::Codex => {
            let program = launch::find_program("codex")
                .context("`codex` 명령을 찾을 수 없습니다. Codex CLI를 설치하세요.")?;
            launch::codex_command(&program, &url, &key, args)
        }
    };
    exec(cmd)
}

/// 이 프로세스를 자식으로 대체한다 (Unix). Windows는 자식을 띄우고 끝날 때까지 기다린다.
fn exec(mut cmd: Command) -> Result<ExitCode> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = cmd.exec();
        Err(err).context("실행하지 못했습니다")
    }
    #[cfg(not(unix))]
    {
        ignore_ctrl_c();
        let status = cmd.status().context("실행하지 못했습니다")?;
        Ok(exit_code(status.code()))
    }
}

fn exit_code(code: Option<i32>) -> ExitCode {
    ExitCode::from(code.map_or(1, |c| u8::try_from(c).unwrap_or(1)))
}

fn verify() -> Result<ExitCode> {
    let settings = Settings::load()?;
    let rt = runtime::ensure(&mut |m| progress(m))?;
    let script = rt.dir.join("verify_api.py");
    std::fs::write(&script, assets::VERIFY_API)?;
    let status = Command::new(rt.python())
        .arg(&script)
        .env("EXAONE_API_KEY", secrets::require_api_key()?)
        .env("EXAONE_API_BASE", &settings.api_base)
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .status()?;
    Ok(exit_code(status.code()))
}

fn doctor() -> Result<ExitCode> {
    let settings = Settings::load()?;
    println!("데이터 폴더: {}\n", paths::data_dir()?.display());
    let checks = doctor::run(&settings);
    for c in &checks {
        let mark = match c.level {
            Level::Ok => "✓",
            Level::Warn => "!",
            Level::Fail => "✗",
        };
        println!("{mark} {} {}", pad(c.name, 16), c.detail);
    }
    let failed = checks.iter().any(|c| c.level == Level::Fail);
    Ok(if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// 한글처럼 화면에서 두 칸을 차지하는 글자를 감안해 폭을 맞춘다.
fn pad(s: &str, width: usize) -> String {
    let w: usize = s.chars().map(|c| if c.is_ascii() { 1 } else { 2 }).sum();
    format!("{s}{}", " ".repeat(width.saturating_sub(w)))
}
