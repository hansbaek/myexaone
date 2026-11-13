# EXAONE 게이트웨이

LG K-EXAONE 2.5 API를 Claude Code, Codex CLI, Antigravity에서 쓰기 위한 로컬 구성입니다.

EXAONE은 OpenAI Chat Completions 형식(`/v1/chat/completions`)만 제공합니다. Claude Code는 Anthropic Messages, Codex는 OpenAI Responses 형식을 요구하므로, 로컬 LiteLLM 게이트웨이가 중간에서 변환합니다. Antigravity는 메인 모델을 바꿀 수 없어 MCP 도구로 연결합니다.

```
Claude Code ── /v1/messages ──┐
Codex CLI ──── /v1/responses ─┼─ 게이트웨이(127.0.0.1:4000) ── /v1/chat/completions ── api.exaone.ai
그 밖의 도구 ─ /v1/chat/... ──┘
Antigravity ── MCP(exaone_chat) ─────────────────────────────────────────────────────── api.exaone.ai
```

## 처음 한 번

```bash
bin/setup
```

`.env`가 만들어집니다. 키는 아래 명령으로 넣습니다(화면에 표시되지 않음). `.env`를 직접 열어 `EXAONE_API_KEY=` 뒤에 넣어도 됩니다.

```bash
bin/set-key
```

그다음 실제 API가 도구 호출을 지원하는지 확인합니다.

```bash
bin/verify-api
```

항목별 통과/실패와 rate limit 헤더가 출력됩니다. 2026-11-13 기준 10개 항목(도구 호출, 병렬 호출, 스트리밍 도구 호출, JSON 스키마 출력 등)이 모두 통과했습니다. "도구 호출"과 "도구 결과를 넣은 후속 응답"이 실패하면 Claude Code와 Codex는 EXAONE으로 동작하지 않습니다(MCP 방식만 가능).

## 사용

게이트웨이를 터미널 하나에 띄워 둡니다.

```bash
bin/gateway
```

다른 터미널에서 작업할 폴더로 이동한 뒤 실행합니다. 인자는 원래 명령에 그대로 전달됩니다.

```bash
/이/폴더/bin/exaone-claude
```

```bash
/이/폴더/bin/exaone-codex
```

두 래퍼 모두 해당 프로세스의 설정만 바꿉니다. 평소의 `claude`, `codex` 설정과 로그인은 그대로입니다.

### Antigravity (MCP)

Antigravity의 MCP 서버 설정(원본 JSON 편집)에 아래를 추가합니다. 경로는 이 폴더의 절대 경로로 바꾸세요. 게이트웨이를 띄울 필요는 없습니다.

```json
{
  "mcpServers": {
    "exaone": { "command": "/절대/경로/bin/exaone-mcp" }
  }
}
```

에이전트가 `exaone_chat` 도구로 EXAONE에게 질문이나 작업을 위임할 수 있습니다. 같은 서버를 Claude Code(`claude mcp add exaone /절대/경로/bin/exaone-mcp`)나 Codex에도 등록할 수 있습니다.

## 파일

| 파일 | 역할 |
|---|---|
| `gateway/config.yaml` | LiteLLM 설정. 모델 등록, 권장 샘플링 값, 재시도, 버릴 파라미터 |
| `bin/set-key` | API 키를 화면 표시 없이 `.env`에 저장 |
| `bin/gateway` | 게이트웨이 실행 |
| `bin/exaone-claude`, `bin/exaone-codex` | 각 도구를 게이트웨이에 붙여 실행 |
| `bin/exaone-mcp`, `mcp/exaone_mcp.py` | MCP 서버 |
| `bin/verify-api`, `tests/verify_api.py` | 실제 API 기능 점검 |
| `tests/mock_upstream.py` | 키 없이 변환 경로를 시험하는 가짜 EXAONE 서버 |

## 설정에서 알아둘 점

- `gateway/config.yaml`의 제공자는 `hosted_vllm/`이고 `use_chat_completions_api: true`가 켜져 있습니다. `openai/`로 바꾸면 LiteLLM이 업스트림에 `/v1/responses`로 보내 실패합니다.
- Claude Code와 Codex가 `claude-*`, `gpt-*` 이름으로 보내는 요청도 전부 EXAONE으로 갑니다.
- `rpm: 60`은 실제 응답 헤더(초당 1회, 분당 60회, 하루 86,400회)와 같은 값입니다.
- 개인 한도와 별개로, 서버 전체가 붐비면 `service is busy, too many concurrent requests(600)` 429가 옵니다. 게이트웨이는 6번까지 재시도하지만 붐비는 시간대에는 응답이 느려지거나 실패할 수 있습니다.
- `reasoning_effort`는 도구가 보내는 값이 그대로 전달됩니다. API 문서는 `low`/`high`/`max`만 적고 있어, `medium`이 거부되는지는 `bin/verify-api`로 확인됩니다.

## 한계

- Claude Code 시작 시 `[claude-code:unrecognized_model]` 한 줄이 출력됩니다. 모르는 모델 이름이라 나오는 안내이며 동작에는 영향이 없습니다.
- 간단한 작업(파일 하나 작성 후 실행)에 Claude Code 약 40초, Codex 약 66초가 걸렸습니다.
- K-EXAONE 2.5는 텍스트 전용이라 이미지 첨부와 스크린샷 기능은 동작하지 않습니다.
- 추론을 끌 수 없어 짧은 호출도 느릴 수 있습니다.
- 한도 상향을 위해 학습 활용에 동의하면 작업 중인 코드가 학습 데이터로 쓰일 수 있습니다.
