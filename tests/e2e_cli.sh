#!/usr/bin/env bash
# `exaone` 명령을 가짜 업스트림으로 끝까지 시험한다 (API 키 불필요).
# macOS와 Windows(Git Bash) 공용. CI에서 쓰지만 로컬에서도 돌릴 수 있다.
#
#   setup → gateway → claude -p → codex exec → mcp → doctor
#
# 사용: bash tests/e2e_cli.sh [exaone 실행 파일]   (기본: target/release/exaone)
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
EXAONE="${1:-$ROOT/target/release/exaone}"
WORK="${RUNNER_TEMP:-$(mktemp -d)}/exaone-e2e-$$"
mkdir -p "$WORK"
if command -v cygpath >/dev/null; then
  WORK_NATIVE="$(cygpath -w "$WORK")"
else
  WORK_NATIVE="$WORK"
fi

# 키 저장소 대신 환경 변수를 쓰고, 데이터 폴더도 임시 폴더로 분리한다.
export EXAONE_STUDIO_HOME="$WORK_NATIVE"
export EXAONE_API_KEY=mock-key
export PYTHONUTF8=1
export LITELLM_MASTER_KEY=sk-e2e-test
UP_PORT=4999
export GATEWAY_PORT=4010
export EXAONE_API_BASE="http://127.0.0.1:$UP_PORT/v1"

PIDS=()
cleanup() {
  for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null; done
  # Windows에서는 exaone.exe 를 꺼도 자식 게이트웨이가 남으므로 포트로 찾아 끈다.
  if command -v taskkill >/dev/null; then
    for pid in $(netstat -ano 2>/dev/null | grep ":$GATEWAY_PORT .*LISTENING" | awk '{print $NF}' | sort -u); do
      taskkill //PID "$pid" //T //F >/dev/null 2>&1
    done
  fi
}
trap cleanup EXIT

FAILED=0
pass() { echo "[PASS] $1"; }
fail() { echo "[FAIL] $1"; echo "$2" | tail -20; FAILED=1; }

echo "== setup"
"$EXAONE" setup --no-key || { echo "setup 실패"; exit 1; }

if [ -f "$WORK/runtime/venv/Scripts/python.exe" ]; then
  PY="$WORK/runtime/venv/Scripts/python.exe"
else
  PY="$WORK/runtime/venv/bin/python"
fi

echo "== 가짜 업스트림과 게이트웨이 시작"
"$PY" "$ROOT/tests/mock_upstream.py" "$UP_PORT" "$WORK_NATIVE/up.jsonl" &
PIDS+=($!)
"$EXAONE" gateway > "$WORK/gateway.log" 2>&1 &
PIDS+=($!)
for _ in $(seq 1 180); do
  curl -s -o /dev/null --max-time 2 "http://127.0.0.1:$GATEWAY_PORT/health/liveliness" && break
  sleep 1
done
if ! curl -s -o /dev/null --max-time 2 "http://127.0.0.1:$GATEWAY_PORT/health/liveliness"; then
  tail -40 "$WORK/gateway.log"
  echo "[FAIL] 게이트웨이가 시작되지 않았습니다"
  exit 1
fi
pass "exaone gateway"

CW="$WORK/work"
mkdir -p "$CW"
cd "$CW" || exit 1

if command -v claude >/dev/null; then
  out="$("$EXAONE" claude -p "hi" < /dev/null 2>&1)"
  if echo "$out" | grep -q MOCK_OK; then pass "exaone claude"; else fail "exaone claude" "$out"; fi
else
  echo "[SKIP] claude 없음"
fi

if command -v codex >/dev/null; then
  out="$("$EXAONE" codex exec --skip-git-repo-check "hi" < /dev/null 2>&1)"
  if echo "$out" | grep -q MOCK_OK; then pass "exaone codex"; else fail "exaone codex" "$out"; fi
else
  echo "[SKIP] codex 없음"
fi

out="$(printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"e2e","version":"1"}}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"exaone_chat","arguments":{"prompt":"hi"}}}' \
  | "$EXAONE" mcp 2>&1)"
if echo "$out" | grep -q '"text":"MOCK_OK"'; then pass "exaone mcp"; else fail "exaone mcp" "$out"; fi

echo "== doctor"
"$EXAONE" doctor || true

echo "== 게이트웨이가 받은 요청"
grep -oE '"(POST|GET) /v1/[a-z]+' "$WORK/gateway.log" | sort | uniq -c

exit $FAILED
