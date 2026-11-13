//! 자식 프로세스 공통 처리 (macOS·Windows).

use std::process::{Child, Command};
use std::time::{Duration, Instant};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 화면 없이 뒤에서 돌 프로세스로 설정한다.
///
/// - Windows: 콘솔 창이 뜨지 않게 한다 (앱에서 띄울 때 검은 창이 깜박이는 것 방지).
/// - Unix: 자기 프로세스 그룹을 갖게 해서, 끌 때 자식까지 한 번에 끌 수 있게 한다.
pub fn background(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd
}

/// 프로세스와 그 자식들을 끈다. 먼저 정상 종료를 요청하고, `grace` 안에 안 끝나면 강제로 끈다.
///
/// `background` 로 띄운 프로세스에 쓴다 (Unix는 프로세스 그룹 단위로 신호를 보낸다).
pub fn kill_tree(child: &mut Child, grace: Duration) {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }
    let pid = child.id().to_string();
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .args(["-TERM", &format!("-{pid}")])
            .status();
        let start = Instant::now();
        while start.elapsed() < grace {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = Command::new("kill")
            .args(["-KILL", &format!("-{pid}")])
            .status();
    }
    #[cfg(windows)]
    {
        // Windows 콘솔 없는 프로세스에는 정상 종료 신호를 보낼 방법이 마땅치 않아 트리째 끈다.
        let _ = grace;
        let _ = Instant::now();
        let mut tk = Command::new("taskkill");
        tk.args(["/PID", &pid, "/T", "/F"]);
        let _ = background(&mut tk).status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn kill_tree_stops_process_group() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "sleep 30 & sleep 30"]);
        let mut child = background(&mut cmd).spawn().unwrap();
        std::thread::sleep(Duration::from_millis(200));
        kill_tree(&mut child, Duration::from_secs(2));
        assert!(child.try_wait().unwrap().is_some());
    }
}
