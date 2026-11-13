"""EXAONE 흉내를 내는 가짜 업스트림. 키 없이 게이트웨이 변환 경로를 시험하는 용도.

- 받은 요청 본문을 전부 로그 파일(JSONL)에 남긴다.
- user 메시지에 `MOCK_TOOL <이름> <JSON인자> END` 가 있으면 그 도구 호출을 돌려준다.
- 대화에 tool 결과가 이미 있으면 "MOCK_DONE", 그 밖에는 "MOCK_OK" 를 돌려준다.

사용: python mock_upstream.py <port> <log.jsonl>
"""
import json
import re
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PORT = int(sys.argv[1])
LOG = sys.argv[2]
MARK = re.compile(r"MOCK_TOOL (\S+) (\{.*?\}) END", re.S)


def text_of(content):
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return "\n".join(p.get("text", "") for p in content if isinstance(p, dict))
    return ""


def decide(body):
    msgs = body.get("messages", [])
    if any(m.get("role") == "tool" for m in msgs):
        return "MOCK_DONE", None
    for msg in msgs:
        if msg.get("role") == "user":
            m = MARK.search(text_of(msg.get("content")))
            if m and body.get("tools"):
                return None, {"name": m.group(1), "arguments": m.group(2)}
    return "MOCK_OK", None


class H(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def do_POST(self):
        raw = self.rfile.read(int(self.headers.get("content-length", 0)))
        body = json.loads(raw or b"{}")
        with open(LOG, "a", encoding="utf-8") as f:
            f.write(json.dumps({"path": self.path, "auth": bool(self.headers.get("authorization")), "body": body}, ensure_ascii=False) + "\n")
        text, tool = decide(body)
        cid, now, model = "chatcmpl-mock", int(time.time()), body.get("model", "k-exaone_v2.5")
        usage = {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15, "reasoning_tokens": 3}
        call = tool and {"id": "call_mock_1", "type": "function", "function": tool}
        finish = "tool_calls" if tool else "stop"

        if not body.get("stream"):
            msg = {"role": "assistant", "content": text, "reasoning_content": "mock reasoning"}
            if call:
                msg["tool_calls"] = [call]
            out = {"id": cid, "object": "chat.completion", "created": now, "model": model,
                   "choices": [{"index": 0, "message": msg, "finish_reason": finish}], "usage": usage}
            data = json.dumps(out).encode()
            self.send_response(200)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
            return

        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.end_headers()

        def chunk(delta, fin=None, **extra):
            d = {"id": cid, "object": "chat.completion.chunk", "created": now, "model": model,
                 "choices": [{"index": 0, "delta": delta, "finish_reason": fin}], **extra}
            self.wfile.write(f"data: {json.dumps(d)}\n\n".encode())
            self.wfile.flush()

        chunk({"role": "assistant", "content": ""})
        chunk({"reasoning_content": "mock reasoning"})
        if call:
            chunk({"tool_calls": [{"index": 0, "id": call["id"], "type": "function",
                                   "function": {"name": tool["name"], "arguments": ""}}]})
            args = tool["arguments"]
            half = len(args) // 2
            for part in (args[:half], args[half:]):
                chunk({"tool_calls": [{"index": 0, "function": {"arguments": part}}]})
        else:
            chunk({"content": "\n\n" + text})
        chunk({}, finish)
        if (body.get("stream_options") or {}).get("include_usage"):
            d = {"id": cid, "object": "chat.completion.chunk", "created": now, "model": model, "choices": [], "usage": usage}
            self.wfile.write(f"data: {json.dumps(d)}\n\n".encode())
        self.wfile.write(b"data: [DONE]\n\n")
        self.wfile.flush()


ThreadingHTTPServer(("127.0.0.1", PORT), H).serve_forever()
