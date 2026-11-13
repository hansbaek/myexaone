//! 가상 터미널(PTY)에서 프로그램을 실행한다. 앱 안 터미널 탭이 쓴다 (macOS·Windows ConPTY).

use std::io::{Read, Write};
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};

/// 세션에서 일어나는 일을 받는 쪽.
pub trait PtyEvents: Send + Sync + 'static {
    /// 화면에 쓸 출력 (UTF-8 글자 경계에 맞춰 잘라서 준다).
    fn output(&self, text: String);
    /// 프로그램이 끝남.
    fn exit(&self, code: u32);
}

/// 실행 중인 터미널 세션. 버리면(drop) 프로그램을 끈다.
pub struct PtySession {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows: rows.max(2),
        cols: cols.max(2),
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// `std::process::Command` 를 PTY용 명령으로 옮긴다.
fn builder(cmd: &Command, cwd: &Path) -> CommandBuilder {
    let program = cmd.get_program();
    // Windows의 .cmd/.bat (npm으로 설치한 claude, codex)는 cmd.exe 를 거쳐 실행한다.
    let is_script = cfg!(windows)
        && Path::new(program)
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
    let mut b = if is_script {
        let mut b = CommandBuilder::new("cmd.exe");
        b.arg("/c");
        b.arg(program);
        b
    } else {
        CommandBuilder::new(program)
    };
    b.args(cmd.get_args());
    for (k, v) in cmd.get_envs() {
        match v {
            Some(v) => b.env(k, v),
            None => b.env_remove(k),
        }
    }
    b.env("TERM", "xterm-256color");
    b.env("COLORTERM", "truecolor");
    b.cwd(cwd);
    b
}

impl PtySession {
    pub fn spawn(
        cmd: &Command,
        cwd: &Path,
        cols: u16,
        rows: u16,
        events: Arc<dyn PtyEvents>,
    ) -> Result<Self> {
        let pair = native_pty_system()
            .openpty(size(cols, rows))
            .context("터미널을 열지 못했습니다")?;
        let mut child = pair
            .slave
            .spawn_command(builder(cmd, cwd))
            .context("프로그램을 실행하지 못했습니다")?;
        drop(pair.slave);
        let killer = child.clone_killer();
        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;

        // 읽기 스레드가 끝난 뒤에 종료를 알려, 마지막 출력이 종료 알림보다 늦게 가지 않게 한다.
        let done = Arc::new(Mutex::new(None::<u32>));
        let (ev, done_r) = (events.clone(), done.clone());
        let read_thread = std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            let mut pending: Vec<u8> = Vec::new();
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        pending.extend_from_slice(&buf[..n]);
                        let text = take_utf8(&mut pending);
                        if !text.is_empty() {
                            ev.output(text);
                        }
                    }
                }
            }
            if !pending.is_empty() {
                ev.output(String::from_utf8_lossy(&pending).into_owned());
            }
            let _ = done_r;
        });
        std::thread::spawn(move || {
            let code = child.wait().map(|s| s.exit_code()).unwrap_or(1);
            *done.lock().unwrap_or_else(|e| e.into_inner()) = Some(code);
            // Windows ConPTY는 프로그램이 끝나도 읽기가 바로 끝나지 않을 수 있어 잠깐만 기다린다.
            let start = std::time::Instant::now();
            while !read_thread.is_finished()
                && start.elapsed() < std::time::Duration::from_millis(1500)
            {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            events.exit(code);
        });

        Ok(Self {
            master: pair.master,
            writer,
            killer,
        })
    }

    /// 키 입력을 보낸다.
    pub fn write(&mut self, data: &str) -> Result<()> {
        self.writer.write_all(data.as_bytes())?;
        self.writer.flush()?;
        Ok(())
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.master
            .resize(size(cols, rows))
            .context("터미널 크기를 바꾸지 못했습니다")
    }

    pub fn kill(&mut self) {
        let _ = self.killer.kill();
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        self.kill();
    }
}

/// 버퍼에서 온전한 UTF-8 부분만 꺼낸다. 잘린 글자의 앞부분은 버퍼에 남긴다.
fn take_utf8(pending: &mut Vec<u8>) -> String {
    match std::str::from_utf8(pending) {
        Ok(s) => {
            let out = s.to_string();
            pending.clear();
            out
        }
        Err(e) => {
            let valid = e.valid_up_to();
            if e.error_len().is_some() {
                // 잘못된 바이트: 통째로 바꿔서 내보낸다.
                let out = String::from_utf8_lossy(pending).into_owned();
                pending.clear();
                out
            } else {
                let rest = pending.split_off(valid);
                String::from_utf8(std::mem::replace(pending, rest)).unwrap_or_default()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn take_utf8_keeps_split_character() {
        let bytes = "한글".as_bytes();
        let mut pending = bytes[..4].to_vec(); // '한' + '글'의 첫 바이트
        assert_eq!(take_utf8(&mut pending), "한");
        assert_eq!(pending.len(), 1);
        pending.extend_from_slice(&bytes[4..]);
        assert_eq!(take_utf8(&mut pending), "글");
        assert!(pending.is_empty());
    }

    struct Collect(Mutex<String>, Mutex<mpsc::Sender<u32>>);
    impl PtyEvents for Collect {
        fn output(&self, text: String) {
            self.0.lock().unwrap().push_str(&text);
        }
        fn exit(&self, code: u32) {
            let _ = self.1.lock().unwrap().send(code);
        }
    }

    #[test]
    fn runs_program_and_reports_output_and_exit() {
        let (tx, rx) = mpsc::channel();
        let events = Arc::new(Collect(Mutex::new(String::new()), Mutex::new(tx)));
        let mut cmd = if cfg!(windows) {
            let mut c = Command::new("cmd.exe");
            c.args(["/c", "echo pty-hello %EXAONE_PTY_TEST%"]);
            c
        } else {
            let mut c = Command::new("sh");
            c.args(["-c", "echo pty-hello $EXAONE_PTY_TEST"]);
            c
        };
        cmd.env("EXAONE_PTY_TEST", "env-ok");
        let dir = std::env::temp_dir();
        let mut session = PtySession::spawn(&cmd, &dir, 80, 24, events.clone()).unwrap();
        // Windows ConPTY는 시작할 때 커서 위치를 묻고(ESC[6n) 답을 기다린다. 앱에서는 xterm.js가 답한다.
        let mut answered = false;
        let start = std::time::Instant::now();
        let code = loop {
            if let Ok(code) = rx.recv_timeout(Duration::from_millis(50)) {
                break code;
            }
            if !answered && events.0.lock().unwrap().contains("\x1b[6n") {
                session.write("\x1b[1;1R").unwrap();
                answered = true;
            }
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "프로그램이 끝나야 한다"
            );
        };
        assert_eq!(code, 0);
        let out = events.0.lock().unwrap().clone();
        assert!(out.contains("pty-hello env-ok"), "출력: {out:?}");
    }
}
