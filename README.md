# EXAONE Studio

LG K-EXAONE 2.5를 Claude Code, Codex, Antigravity에서 쓰게 해 주는 macOS·Windows 앱과 명령줄 도구입니다.

EXAONE API는 OpenAI Chat Completions 형식(`/v1/chat/completions`)만 제공합니다. Claude Code는 Anthropic Messages, Codex는 OpenAI Responses 형식을 쓰므로, 이 컴퓨터에서 도는 LiteLLM 게이트웨이가 중간에서 변환합니다. 메인 모델을 바꿀 수 없는 도구(Antigravity 등)에는 EXAONE을 `exaone_chat` MCP 도구로 붙입니다.

```
Claude Code ── /v1/messages ──┐
Codex ──────── /v1/responses ─┼─ 게이트웨이(127.0.0.1:4000) ── /v1/chat/completions ── api.exaone.ai
대화 창 ────── /v1/chat/... ──┘
Antigravity 등 ── MCP(exaone_chat) ─────────────────────────────────────────────────── api.exaone.ai
```

## 설치

[Releases](https://github.com/hansbaek/myexaone/releases)에서 받습니다.

| 파일 | 대상 |
|---|---|
| `EXAONE Studio_*_aarch64.dmg` | macOS (Apple Silicon) |
| `EXAONE Studio_*_x64-setup.exe` 또는 `.msi` | Windows |
| `exaone-*.tar.gz` / `.zip` | 명령줄 도구 (선택) |

앱만 설치하면 됩니다. 처음 켤 때 실행 환경(LiteLLM, 약 500MB)을 스스로 내려받습니다. Claude Code·Codex를 EXAONE으로 쓰려면 그 도구들은 따로 설치돼 있어야 합니다.

서명되지 않은 앱이라 처음 열 때 경고가 나옵니다.

- **macOS**: 앱을 응용 프로그램 폴더로 옮긴 뒤 우클릭 → 열기. 그래도 열리지 않으면 아래 명령을 실행합니다.

  ```bash
  xattr -dr com.apple.quarantine "/Applications/EXAONE Studio.app"
  ```

- **Windows**: SmartScreen 창에서 "추가 정보" → "실행"

## 사용

1. 앱을 열고 **설정**에서 EXAONE API 키를 넣습니다. 키는 OS 키 저장소(macOS 키체인, Windows 자격 증명 관리자)에만 저장됩니다.
2. **대시보드**에서 게이트웨이가 "실행 중"이 되면 준비 끝입니다. 이후에는 앱을 켜면 게이트웨이도 자동으로 켜집니다.

| 탭 | 하는 일 |
|---|---|
| 대시보드 | 게이트웨이 켜기·끄기, 진단 (키, 실행 환경, 포트, Claude 설정 충돌, 예전 설정으로 실행 중인지 등) |
| 대화 | EXAONE과 바로 대화. 추론 과정 접기, 대기·생성 시간 표시, 서버 혼잡 시 자동 재시도 |
| 세션 | 작업 폴더를 고르면 앱 안 터미널에서 Claude Code나 Codex를 EXAONE으로 실행 |
| 사용량 | 출처별(Claude Code·Codex·대화 창)·날짜별 토큰, 서버 혼잡(429) 비율, 평균 응답 시간 |
| 도구 연결 | Claude Code·Codex에 `exaone_chat` MCP 도구 등록·해제, Antigravity 등에 붙여 넣을 설정 |
| 설정 | API 키, 로그인 시 자동 실행, 한글 입력 처리 방식 |

- 창을 닫아도 앱은 메뉴 막대(Windows는 알림 영역)에 남아 게이트웨이를 계속 돌립니다. 완전히 끄려면 아이콘 메뉴의 "종료"를 누릅니다.
- 평소의 `claude`, `codex` 설정과 로그인은 바뀌지 않습니다. EXAONE은 앱의 세션 탭이나 `exaone claude`로 실행할 때만 씁니다.
- 앱을 다른 폴더로 옮겼다면 도구 연결 탭에서 "다시 등록"을 누르세요.

## 명령줄 도구 `exaone`

앱 없이 터미널에서만 쓸 때, 또는 앱이 띄운 게이트웨이에 터미널에서 붙을 때 씁니다.

```bash
exaone setup
```

```bash
exaone gateway
```

```bash
exaone claude
```

| 명령 | 하는 일 |
|---|---|
| `exaone setup` | 실행 환경 설치, 키가 없으면 입력받기 |
| `exaone key set` / `status` / `delete` | API 키 관리 |
| `exaone gateway [--port N]` | 게이트웨이 실행 (앱이 이미 띄웠으면 그대로 씀) |
| `exaone claude …`, `exaone codex …` | EXAONE으로 실행. 뒤의 인자는 원래 명령에 전달 |
| `exaone mcp` | `exaone_chat` MCP 서버 (stdio) |
| `exaone verify` | 실제 API의 도구 호출·구조화 출력·한도 점검 |
| `exaone doctor` | 환경 진단 |

데이터 폴더는 macOS `~/Library/Application Support/EXAONE Studio`, Windows `%APPDATA%\EXAONE Studio`입니다. 실행 환경, 설정(`settings.json`), 대화·사용량 저장소(`studio.db`), 로그가 여기 들어갑니다.

## 개발

필요한 것: Rust, Node.js 22 이상, [uv](https://docs.astral.sh/uv/).

```bash
cargo test --workspace
```

```bash
cargo build --release
```

```bash
cd app && npm install && npm run tauri build
```

키 없이 변환 경로를 시험할 수 있습니다. 가짜 EXAONE 서버(`tests/mock_upstream.py`)를 씁니다.

```bash
bash tests/e2e_cli.sh
```

GitHub Actions(`build`)가 push 마다 macOS·Windows에서 단위 테스트, 게이트웨이 점검, `exaone` 종단 간 시험(실제 Claude Code·Codex 포함), 앱 빌드를 돌립니다.

### 릴리스

`app/src-tauri/tauri.conf.json`의 `version`을 올리고 같은 버전의 태그를 올리면, `release` 워크플로가 설치 파일과 명령줄 도구를 빌드해 GitHub Release를 만듭니다.

```bash
git tag v0.1.0 && git push origin v0.1.0
```

### 구조

| 경로 | 역할 |
|---|---|
| `crates/exaone-core` | 핵심 로직: 실행 환경(uv·LiteLLM), 키 저장소, 게이트웨이, Claude·Codex 실행, PTY, 대화·사용량 저장소, MCP 서버·등록, 진단 |
| `crates/exaone-cli` | `exaone` 명령 |
| `app/` | 데스크톱 앱 (Tauri 2 + React). `src-tauri`는 core를 화면에 잇는 부분 |
| `gateway/config.yaml` | LiteLLM 설정: 모델, 권장 샘플링 값, 재시도, 버릴 파라미터 |
| `gateway/usage_logger.py` | 요청마다 사용 기록(출처, 토큰, 시간, 재시도 횟수)을 남기는 LiteLLM 콜백. 내용은 남기지 않음 |
| `tests/` | 가짜 EXAONE 서버, 게이트웨이 점검, 종단 간 시험, 실제 API 점검, 한글 입력 재생 자료 |

## 알아둘 점

- **서버 혼잡**: 개인 한도(초당 1회, 분당 60회, 하루 86,400회)와 별개로, 서버 전체가 붐비면 `too many concurrent requests(600)` 429가 자주 옵니다. 게이트웨이가 6번까지 재시도하고 대화 창은 3번 더 시도하지만, 붐비는 시간에는 수십 초 걸리거나 실패할 수 있습니다.
- **추론 강도**: 추론 강도(`reasoning_effort`)를 낮춰도 실제 차이는 추론 수십 토큰(1~2초) 정도입니다. 추론은 끌 수 없습니다.
- **Codex 입력 토큰**: Codex는 요청마다 도구·플러그인 설명을 함께 보내서, 짧은 질문도 입력이 수만~십수만 토큰이 될 수 있습니다. 사용량 탭에서 확인할 수 있습니다.
- **Claude 설정 우선순위**: Claude Code는 셸 환경 변수보다 `~/.claude/settings.json`의 `env`를 우선합니다. 그래서 EXAONE 연결 설정은 `--settings`로 넘깁니다. 그 파일에 다른 `ANTHROPIC_BASE_URL`이 있으면 진단이 알려 줍니다(앱·`exaone claude`는 영향 없음).
- **모델 이름 안내 줄**: Claude Code 시작 시 `[claude-code:unrecognized_model]` 한 줄이 나옵니다. 동작에는 영향이 없습니다.
- **이미지 불가**: K-EXAONE 2.5는 텍스트 전용이라 이미지 첨부는 동작하지 않습니다.
- **학습 활용 동의**: 한도 상향을 위해 학습 활용에 동의하면 작업 중인 코드가 학습 데이터로 쓰일 수 있습니다.
- **키체인 승인 창 (macOS)**: 서명되지 않은 앱을 새 버전으로 바꾸면, macOS가 키체인 접근을 다시 물을 수 있습니다. "항상 허용"을 누르면 됩니다.
