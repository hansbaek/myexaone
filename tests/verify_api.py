"""EXAONE API가 코딩 에이전트에 필요한 기능을 실제로 지원하는지 확인한다.

문서에 없는 항목(tools, tool_choice, response_format 등)을 직접 호출해 보고
항목별로 통과/실패와 rate limit 헤더를 출력한다. `exaone verify` 로 실행.
"""
import json
import os
import sys
import time

import httpx

BASE = os.environ["EXAONE_API_BASE"].rstrip("/")
KEY = os.environ["EXAONE_API_KEY"]
MODEL = "k-exaone_v2.5"
URL = f"{BASE}/chat/completions"
HEADERS = {"Authorization": f"Bearer {KEY}", "Content-Type": "application/json"}
WEATHER = {"type": "function", "function": {
    "name": "get_weather", "description": "도시의 현재 날씨를 조회한다.",
    "parameters": {"type": "object", "properties": {"city": {"type": "string"}}, "required": ["city"]}}}
ASK = [{"role": "user", "content": "get_weather 도구로 서울 날씨를 조회해 줘."}]
results = []
limits = {}


busy_retries = 0


def call(extra, messages=ASK, stream=False):
    """429(서버 혼잡 포함)는 기능 실패가 아니므로 기다렸다 최대 6번 다시 시도한다."""
    global busy_retries
    for attempt in range(7):
        status, data = _call(extra, messages, stream)
        if status != 429:
            return status, data
        busy_retries += 1
        time.sleep(min(5 * 2 ** attempt, 60))
    return status, data


def _call(extra, messages, stream):
    body = {"model": MODEL, "messages": messages, "max_tokens": 2048, "reasoning_effort": "low", **extra}
    time.sleep(1.5)  # 초당 한도 회피
    if not stream:
        r = httpx.post(URL, headers=HEADERS, json=body, timeout=300)
        for k, v in r.headers.items():
            if "rate-limit" in k.lower() or k.lower().startswith("retry-after"):
                limits[k] = v
        return r.status_code, (r.json() if r.headers.get("content-type", "").startswith("application/json") else r.text)
    chunks = []
    with httpx.stream("POST", URL, headers=HEADERS, json={**body, "stream": True}, timeout=300) as r:
        if r.status_code != 200:
            return r.status_code, r.read().decode()
        for line in r.iter_lines():
            if line.startswith("data: ") and line != "data: [DONE]":
                chunks.append(json.loads(line[6:]))
    return 200, chunks


def check(name, fn):
    try:
        ok, detail = fn()
    except Exception as e:  # noqa: BLE001
        ok, detail = False, f"예외: {e!r}"
    results.append((name, ok))
    print(f"[{'통과' if ok else '실패'}] {name}\n        {str(detail)[:300]}")


def msg(data):
    return data["choices"][0]["message"]


def t_basic():
    s, d = call({}, [{"role": "user", "content": "한 단어로 인사해 줘."}])
    return s == 200, (msg(d).get("content") if s == 200 else d)


def t_tools():
    s, d = call({"tools": [WEATHER]})
    calls = s == 200 and msg(d).get("tool_calls")
    return bool(calls), (calls if s == 200 else d)


def t_tool_choice():
    s, d = call({"tools": [WEATHER], "tool_choice": {"type": "function", "function": {"name": "get_weather"}}},
                [{"role": "user", "content": "안녕"}])
    calls = s == 200 and msg(d).get("tool_calls")
    return bool(calls), (calls if s == 200 else d)


def t_parallel():
    s, d = call({"tools": [WEATHER], "parallel_tool_calls": True},
                [{"role": "user", "content": "get_weather 도구로 서울과 부산 날씨를 한 번에 조회해 줘."}])
    calls = (s == 200 and msg(d).get("tool_calls")) or []
    return len(calls) >= 2, (f"{len(calls)}개 호출" if s == 200 else d)


def t_roundtrip():
    history = ASK + [
        {"role": "assistant", "content": None, "tool_calls": [
            {"id": "call_1", "type": "function", "function": {"name": "get_weather", "arguments": "{\"city\": \"서울\"}"}}]},
        {"role": "tool", "tool_call_id": "call_1", "content": "맑음, 12도"},
    ]
    s, d = call({"tools": [WEATHER]}, history)
    return s == 200 and bool(msg(d).get("content")), (msg(d).get("content") if s == 200 else d)


def t_stream_tools():
    s, chunks = call({"tools": [WEATHER], "stream_options": {"include_usage": True}}, stream=True)
    if s != 200:
        return False, chunks
    args = "".join(tc["function"].get("arguments") or "" for c in chunks for ch in c.get("choices", [])
                   for tc in (ch["delta"].get("tool_calls") or []))
    return bool(args), f"스트리밍된 인자: {args}"


def t_json_schema():
    schema = {"type": "json_schema", "json_schema": {"name": "city", "strict": True, "schema": {
        "type": "object", "properties": {"city": {"type": "string"}}, "required": ["city"], "additionalProperties": False}}}
    s, d = call({"response_format": schema}, [{"role": "user", "content": "한국의 수도를 JSON으로 답해 줘."}])
    if s != 200:
        return False, d
    text = (msg(d).get("content") or "").strip()
    try:
        return "city" in json.loads(text), text
    except ValueError:
        return False, text


def t_effort_medium():
    s, d = call({"reasoning_effort": "medium"}, [{"role": "user", "content": "1+1?"}])
    return s == 200, (msg(d).get("content") if s == 200 else d)


def t_two_systems():
    s, d = call({}, [{"role": "system", "content": "너는 간결하다."}, {"role": "system", "content": "한국어로 답한다."},
                     {"role": "user", "content": "1+1?"}])
    return s == 200, (msg(d).get("content") if s == 200 else d)


def t_unknown_param():
    s, d = call({"prompt_cache_key": "abc", "store": False}, [{"role": "user", "content": "1+1?"}])
    return s == 200, (msg(d).get("content") if s == 200 else d)


check("기본 대화", t_basic)
check("도구 호출 (tools)", t_tools)
check("도구 강제 지정 (tool_choice)", t_tool_choice)
check("병렬 도구 호출", t_parallel)
check("도구 결과를 넣은 후속 응답", t_roundtrip)
check("스트리밍 중 도구 호출", t_stream_tools)
check("구조화 출력 (response_format json_schema)", t_json_schema)
check("reasoning_effort=medium 허용", t_effort_medium)
check("system 메시지 2개 허용", t_two_systems)
check("모르는 파라미터 무시", t_unknown_param)

print(f"\n429로 재시도한 횟수: {busy_retries}")
print("rate limit 헤더:", json.dumps(limits, ensure_ascii=False) if limits else "응답에 없음")
failed = [n for n, ok in results if not ok]
print(f"\n{len(results) - len(failed)}/{len(results)} 통과" + (f" · 실패: {', '.join(failed)}" if failed else ""))
sys.exit(1 if failed else 0)
