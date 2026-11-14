# EXAONE 게이트웨이

LG K-EXAONE 2.5 API를 Claude Code, Codex CLI, Antigravity에서 쓰기 위한 로컬 구성입니다.

EXAONE은 OpenAI Chat Completions 형식(`/v1/chat/completions`)만 제공합니다. Claude Code는 Anthropic Messages, Codex는 OpenAI Responses 형식을 요구하므로, 로컬 LiteLLM 게이트웨이가 중간에서 변환합니다. Antigravity는 메인 모델을 바꿀 수 없어 MCP 도구로 연결합니다.

```
Claude Code ── /v1/messages ──┐
Codex CLI ──── /v1/responses ─┼─ 게이트웨이(127.0.0.1:4000) ── /v1/chat/completions ── api.exaone.ai
그 밖의 도구 ─ /v1/chat/... ──┘
Antigravity ── MCP(exaone_chat) ─────────────────────────────────────────────────────── api.exaone.ai
```

## 데스크톱 앱 (EXAONE Studio)

게이트웨이를 켜고 끄는 일을 앱이 대신합니다. 트레이(메뉴 막대)에 상주하며, 창을 닫아도 게이트웨이는 계속 돕니다.

- 앱을 켜면 게이트웨이가 자동으로 켜지고, 앱을 종료하면 함께 꺼집니다.
- API 키는 설정 화면에서 입력합니다(OS 키 저장소에 저장).
- uv가 앱에 들어 있어, 처음 켤 때 실행 환경(LiteLLM, 약 500MB)을 스스로 설치합니다.
- 로그와 진단을 화면에서 볼 수 있습니다.
- 사용량 탭에서 게이트웨이를 거친 요청(Claude Code·Codex 세션, 대화 창)의 출처별·날짜별 토큰, 서버 혼잡(429) 비율, 평균 응답 시간을 봅니다. 기록에는 질문·답변 내용이 들어가지 않습니다.
- 대화 탭에서 EXAONE과 바로 대화합니다. 답변은 스트리밍으로 오고, 추론 과정은 접어서 보여 줍니다. 대화 기록은 데이터 폴더의 `studio.db`(SQLite)에만 저장됩니다.

빌드는 `app/`에서 합니다. 결과물은 `app/src-tauri/target/release/bundle/`에 생깁니다.

```bash
cd app && npm install && npm run tauri build
```

세션 탭에서 도구(Claude Code 또는 Codex)와 작업 폴더를 고르면 앱 안 터미널에서 바로 실행됩니다. 탭을 여러 개 열 수 있고, 최근 폴더를 기억합니다. 터미널에서 `exaone claude`, `exaone codex`로 실행해도 됩니다.

## 명령줄 도구 설치 (macOS·Windows)

`exaone` 명령 하나로 게이트웨이와 Claude Code·Codex를 다룹니다. 빌드하려면 Rust와 [uv](https://docs.astral.sh/uv/)가 필요합니다.

```bash
cargo build --release
```

실행 파일은 `target/release/exaone`(Windows는 `exaone.exe`)입니다. PATH에 있는 폴더로 복사해 두면 어디서나 `exaone`으로 실행할 수 있습니다. GitHub Actions 실행 결과의 `exaone-cli-macOS`, `exaone-cli-Windows`에서 빌드된 파일을 받을 수도 있습니다.

처음 한 번, 실행 환경(LiteLLM)을 설치하고 API 키를 저장합니다. 키는 OS 키 저장소(macOS 키체인, Windows 자격 증명 관리자)에 들어가며 화면에 표시되지 않습니다.

```bash
exaone setup
```

실제 API가 도구 호출을 지원하는지 확인하려면 다음을 실행합니다. 2026-11-13 기준 10개 항목이 모두 통과했습니다.

```bash
exaone verify
```

## 사용

게이트웨이를 터미널 하나에 띄워 둡니다(Ctrl+C로 종료).

```bash
exaone gateway
```

다른 터미널에서 작업할 폴더로 이동한 뒤 실행합니다. 뒤의 인자는 원래 명령에 그대로 전달됩니다.

```bash
exaone claude
```

```bash
exaone codex
```

평소의 `claude`, `codex` 설정과 로그인은 바뀌지 않습니다. 연결이 안 되면 먼저 진단을 실행하세요. 실행 환경, 키, 게이트웨이, `claude`·`codex` 위치, Claude 설정 충돌을 한 번에 보여 줍니다.

```bash
exaone doctor
```

| 명령 | 하는 일 |
|---|---|
| `exaone setup` | 실행 환경 설치, 키가 없으면 입력받기 |
| `exaone key set` / `status` / `delete` | API 키 관리 |
| `exaone gateway [--port N]` | 게이트웨이 실행 |
| `exaone claude …`, `exaone codex …` | EXAONE으로 실행 |
| `exaone mcp` | `exaone_chat` MCP 서버 (stdio) |
| `exaone verify` | 실제 API 기능 점검 |
| `exaone doctor` | 환경 진단 |

데이터 폴더는 macOS `~/Library/Application Support/EXAONE Studio`, Windows `%APPDATA%\EXAONE Studio`입니다. 설정(`settings.json`의 `gateway_port`, `api_base`)과 실행 환경이 여기 들어갑니다.

### MCP (Antigravity 등)

MCP 클라이언트 설정에 아래를 추가합니다. 게이트웨이를 띄울 필요는 없습니다.

```json
{
  "mcpServers": {
    "exaone": { "command": "exaone", "args": ["mcp"] }
  }
}
```

PATH에 `exaone`이 없으면 `command`에 실행 파일의 절대 경로를 씁니다. Claude Code에는 `claude mcp add --scope user exaone -- exaone mcp`로 등록합니다.

### 기존 맥 전용 스크립트

`bin/` 아래 bash 스크립트(`bin/gateway`, `bin/exaone-claude` 등)와 `mcp/exaone_mcp.py`도 그대로 동작합니다. 이 스크립트들은 `.env`와 저장소 안의 `.venv`를 씁니다. 같은 일을 `exaone` 명령이 대신하므로 나중에 정리할 예정입니다.

## 파일

| 파일 | 역할 |
|---|---|
| `crates/exaone-core` | 핵심 로직: 실행 환경, 키 저장소, 게이트웨이, Claude·Codex 실행, MCP, 진단 |
| `crates/exaone-cli` | `exaone` 명령 |
| `app/` | 데스크톱 앱 (Tauri 2, 개발 중) |
| `gateway/config.yaml` | LiteLLM 설정. 모델 등록, 권장 샘플링 값, 재시도, 버릴 파라미터 |
| `gateway/usage_logger.py` | 요청마다 사용 기록(출처, 토큰, 시간, 재시도 횟수)을 남기는 LiteLLM 콜백 |
| `bin/set-key` | API 키를 화면 표시 없이 `.env`에 저장 |
| `bin/gateway` | 게이트웨이 실행 |
| `bin/exaone-claude`, `bin/exaone-codex` | 각 도구를 게이트웨이에 붙여 실행 |
| `bin/exaone-mcp`, `mcp/exaone_mcp.py` | MCP 서버 |
| `bin/verify-api`, `tests/verify_api.py` | 실제 API 기능 점검 |
| `tests/mock_upstream.py` | 키 없이 변환 경로를 시험하는 가짜 EXAONE 서버 |
| `tests/smoke_gateway.py` | 게이트웨이 변환 경로 점검 (OS 공용) |
| `tests/e2e_cli.sh` | `exaone` 명령 종단 간 시험 (CI) |

## 설정에서 알아둘 점

- `gateway/config.yaml`의 제공자는 `hosted_vllm/`이고 `use_chat_completions_api: true`가 켜져 있습니다. `openai/`로 바꾸면 LiteLLM이 업스트림에 `/v1/responses`로 보내 실패합니다.
- Claude Code와 Codex가 `claude-*`, `gpt-*` 이름으로 보내는 요청도 전부 EXAONE으로 갑니다.
- `rpm: 60`은 실제 응답 헤더(초당 1회, 분당 60회, 하루 86,400회)와 같은 값입니다.
- 개인 한도와 별개로, 서버 전체가 붐비면 `service is busy, too many concurrent requests(600)` 429가 옵니다. 게이트웨이는 6번까지 재시도하지만 붐비는 시간대에는 응답이 느려지거나 실패할 수 있습니다.
- `reasoning_effort`는 도구가 보내는 값이 그대로 전달됩니다. API 문서는 `low`/`high`/`max`만 적고 있어, 실제로는 `medium`도 받습니다(`exaone verify`로 확인).

## 한계

- Claude Code는 셸 환경 변수보다 `~/.claude/settings.json`의 `env` 값을 우선합니다. 그래서 `exaone claude`는 연결 설정을 `--settings`로 넘깁니다. `exaone doctor`가 이 충돌을 알려 줍니다. 그 파일에 다른 도구가 넣은 `ANTHROPIC_BASE_URL`이 있어도 이 래퍼는 게이트웨이로 연결되지만, 평소의 `claude` 명령은 그 주소로 연결됩니다.
- Claude Code 시작 시 `[claude-code:unrecognized_model]` 한 줄이 출력됩니다. 모르는 모델 이름이라 나오는 안내이며 동작에는 영향이 없습니다.
- 간단한 작업(파일 하나 작성 후 실행)에 Claude Code 약 40초, Codex 약 66초가 걸렸습니다.
- K-EXAONE 2.5는 텍스트 전용이라 이미지 첨부와 스크린샷 기능은 동작하지 않습니다.
- 추론을 끌 수 없어 짧은 호출도 느릴 수 있습니다.
- 한도 상향을 위해 학습 활용에 동의하면 작업 중인 코드가 학습 데이터로 쓰일 수 있습니다.
