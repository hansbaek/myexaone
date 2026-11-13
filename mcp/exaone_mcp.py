"""EXAONE을 도구로 노출하는 MCP 서버 (stdio).

메인 모델을 바꿀 수 없는 Antigravity 같은 도구에서, 에이전트가 EXAONE에게
작업을 위임할 수 있게 한다. 게이트웨이 없이 EXAONE API를 직접 호출한다.
"""
import asyncio
import os
from typing import Literal

import httpx
from mcp.server.mcpserver import MCPServer

BASE = os.environ.get("EXAONE_API_BASE", "https://api.exaone.ai/v1").rstrip("/")
KEY = os.environ.get("EXAONE_API_KEY", "")
MODEL = "k-exaone_v2.5"

server = MCPServer("exaone")


@server.tool()
async def exaone_chat(
    prompt: str,
    system: str = "",
    reasoning_effort: Literal["low", "high", "max"] = "high",
    max_tokens: int = 8192,
) -> str:
    """LG K-EXAONE 2.5에게 질문하거나 작업을 맡기고 답변 텍스트를 받는다.

    한국어 글쓰기·번역·요약·검토, 한국 관련 지식, 다른 모델의 두 번째 의견이 필요할 때 쓴다.
    이 호출은 대화 기록을 기억하지 않으므로 필요한 맥락(코드, 문서 등)을 prompt에 모두 넣는다.
    텍스트만 받는다(이미지 불가). 답변까지 수십 초가 걸릴 수 있다.
    """
    if not KEY:
        return "오류: EXAONE_API_KEY 가 설정되지 않았습니다. .env 를 확인하세요."
    messages = ([{"role": "system", "content": system}] if system else []) + [{"role": "user", "content": prompt}]
    body = {"model": MODEL, "messages": messages, "max_tokens": max_tokens,
            "temperature": 1.0, "top_p": 0.95, "reasoning_effort": reasoning_effort}
    # 서버 혼잡 429가 자주 오므로 기다렸다 최대 6번 다시 시도한다.
    async with httpx.AsyncClient(timeout=600) as client:
        for attempt in range(7):
            r = await client.post(f"{BASE}/chat/completions", json=body,
                                  headers={"Authorization": f"Bearer {KEY}"})
            if r.status_code != 429 or attempt == 6:
                break
            await asyncio.sleep(min(2 * 2 ** attempt, 30))
    if r.status_code == 429:
        return "오류: EXAONE 호출 한도 초과 또는 서버 혼잡(429). 잠시 뒤 다시 시도하세요."
    if r.status_code != 200:
        return f"오류: EXAONE API {r.status_code}: {r.text[:500]}"
    return (r.json()["choices"][0]["message"].get("content") or "").strip()


if __name__ == "__main__":
    server.run()
