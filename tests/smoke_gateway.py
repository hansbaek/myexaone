"""게이트웨이 변환 경로 점검 (macOS·Windows 공용, API 키 불필요).

가짜 업스트림(tests/mock_upstream.py)과 LiteLLM 게이트웨이를 띄운 뒤
Claude Code(/v1/messages), Codex(/v1/responses), 일반(/v1/chat/completions)
경로에서 대화·도구 호출·스트리밍이 제대로 변환되는지 확인한다.

사용: python tests/smoke_gateway.py [--litellm <litellm 실행 파일>]
"""
import argparse
import json
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import httpx

ROOT = Path(__file__).resolve().parent.parent
TOOL_PROMPT = 'MOCK_TOOL get_weather {"city":"Seoul"} END'
KEY = "sk-smoke-test"


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def default_litellm():
    venv = Path(sys.executable).parent
    for name in ("litellm", "litellm.exe"):
        if (venv / name).exists():
            return str(venv / name)
    return shutil.which("litellm") or "litellm"


def wait_ready(url, proc, timeout=180):
    end = time.time() + timeout
    while time.time() < end:
        if proc.poll() is not None:
            return False
        try:
            if httpx.get(url, timeout=2).status_code == 200:
                return True
        except httpx.HTTPError:
            pass
        time.sleep(1)
    return False


def sse_events(resp):
    for line in resp.iter_lines():
        if line.startswith("data: ") and line != "data: [DONE]":
            yield json.loads(line[6:])


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--litellm", default=default_litellm())
    args = ap.parse_args()

    tmp = Path(tempfile.mkdtemp())
    up_port, gw_port = free_port(), free_port()
    procs = []
    results = []

    def check(name, fn):
        try:
            ok, detail = fn()
        except Exception as e:  # noqa: BLE001
            ok, detail = False, repr(e)
        results.append(ok)
        print(f"[{'PASS' if ok else 'FAIL'}] {name}: {str(detail)[:200]}", flush=True)

    try:
        procs.append(subprocess.Popen(
            [sys.executable, str(ROOT / "tests" / "mock_upstream.py"), str(up_port), str(tmp / "up.jsonl")]))
        env = {**os.environ,
               "EXAONE_API_KEY": "mock-key",
               "EXAONE_API_BASE": f"http://127.0.0.1:{up_port}/v1",
               "LITELLM_MASTER_KEY": KEY,
               "PYTHONUTF8": "1"}
        log = open(tmp / "gateway.log", "w", encoding="utf-8")
        gw = subprocess.Popen(
            [args.litellm, "--config", str(ROOT / "gateway" / "config.yaml"),
             "--host", "127.0.0.1", "--port", str(gw_port)],
            env=env, stdout=log, stderr=subprocess.STDOUT)
        procs.append(gw)
        base = f"http://127.0.0.1:{gw_port}"
        if not wait_ready(f"{base}/health/liveliness", gw):
            log.close()
            print((tmp / "gateway.log").read_text(encoding="utf-8", errors="replace")[-3000:])
            print("[FAIL] 게이트웨이가 시작되지 않았습니다.")
            return 1
        print(f"게이트웨이 준비 완료 ({sys.platform})", flush=True)

        anth = {"x-api-key": KEY, "anthropic-version": "2023-06-01"}
        oai = {"Authorization": f"Bearer {KEY}"}
        a_tool = {"name": "get_weather", "description": "w",
                  "input_schema": {"type": "object", "properties": {"city": {"type": "string"}}}}
        r_tool = {"type": "function", "name": "get_weather",
                  "parameters": {"type": "object", "properties": {"city": {"type": "string"}}}}

        def messages_plain():
            r = httpx.post(f"{base}/v1/messages", headers=anth, timeout=60, json={
                "model": "claude-sonnet-4-5", "max_tokens": 100,
                "messages": [{"role": "user", "content": "hi"}]})
            texts = [b.get("text") for b in r.json().get("content", []) if b.get("type") == "text"]
            return r.status_code == 200 and "MOCK_OK" in "".join(texts), texts

        def messages_tool():
            r = httpx.post(f"{base}/v1/messages", headers=anth, timeout=60, json={
                "model": "k-exaone", "max_tokens": 100, "tools": [a_tool],
                "messages": [{"role": "user", "content": TOOL_PROMPT}]})
            d = r.json()
            uses = [b for b in d.get("content", []) if b.get("type") == "tool_use"]
            return d.get("stop_reason") == "tool_use" and uses and uses[0]["input"] == {"city": "Seoul"}, uses

        def messages_tool_stream():
            with httpx.stream("POST", f"{base}/v1/messages", headers=anth, timeout=60, json={
                    "model": "k-exaone", "max_tokens": 100, "stream": True, "tools": [a_tool],
                    "messages": [{"role": "user", "content": TOOL_PROMPT}]}) as r:
                ev = list(sse_events(r))
            args_ = "".join(e["delta"]["partial_json"] for e in ev
                            if e.get("type") == "content_block_delta" and e["delta"].get("type") == "input_json_delta")
            return json.loads(args_ or "{}") == {"city": "Seoul"}, args_

        def responses_plain():
            r = httpx.post(f"{base}/v1/responses", headers=oai, timeout=60, json={
                "model": "gpt-5", "input": "hi", "reasoning": {"effort": "medium"}})
            out = r.json().get("output", [])
            texts = [c.get("text") for o in out if o.get("type") == "message" for c in o.get("content", [])]
            return r.status_code == 200 and "MOCK_OK" in "".join(texts), texts

        def responses_tool_stream():
            with httpx.stream("POST", f"{base}/v1/responses", headers=oai, timeout=60, json={
                    "model": "k-exaone", "stream": True, "input": TOOL_PROMPT, "tools": [r_tool]}) as r:
                ev = list(sse_events(r))
            done = [e for e in ev if e.get("type") == "response.function_call_arguments.done"]
            return bool(done) and json.loads(done[0]["arguments"]) == {"city": "Seoul"}, done[:1]

        def chat_plain():
            r = httpx.post(f"{base}/v1/chat/completions", headers=oai, timeout=60, json={
                "model": "k-exaone", "messages": [{"role": "user", "content": "hi"}]})
            text = r.json()["choices"][0]["message"]["content"]
            return text == "MOCK_OK", text

        def upstream_only_chat():
            paths = {json.loads(l)["path"] for l in (tmp / "up.jsonl").read_text(encoding="utf-8").splitlines()}
            return paths == {"/v1/chat/completions"}, paths

        check("Claude Code: 대화 (/v1/messages)", messages_plain)
        check("Claude Code: 도구 호출", messages_tool)
        check("Claude Code: 스트리밍 도구 호출", messages_tool_stream)
        check("Codex: 대화 (/v1/responses)", responses_plain)
        check("Codex: 스트리밍 도구 호출", responses_tool_stream)
        check("OpenAI 호환: 대화 (/v1/chat/completions)", chat_plain)
        check("업스트림은 chat/completions 만 받음", upstream_only_chat)
    finally:
        for p in procs:
            p.terminate()
        for p in procs:
            try:
                p.wait(timeout=10)
            except subprocess.TimeoutExpired:
                p.kill()

    passed = sum(results)
    print(f"\n{passed}/{len(results)} 통과")
    return 0 if results and all(results) else 1


if __name__ == "__main__":
    sys.exit(main())
