"""게이트웨이 사용 기록. 요청 하나가 끝날 때마다 JSON 한 줄을 남긴다.

EXAONE Studio 앱이 이 파일을 읽어 사용량 통계를 만든다.
기록 위치: 환경 변수 EXAONE_USAGE_LOG (없으면 기록하지 않음)

남기는 것: 시각, 출처(경로로 판단), 성공 여부, 상태 코드, 토큰 수, 걸린 시간.
질문·답변 내용은 남기지 않는다.
"""

import json
import os
import threading
import time

from litellm.integrations.custom_logger import CustomLogger

_LOCK = threading.Lock()

# 요청 ID(litellm_call_id, 재시도해도 같다) → [업스트림 시도 횟수, 처음 시도한 시각]. 게이트웨이 안의 재시도를 센다.
_ATTEMPTS: dict = {}

# 들어온 경로로 어느 도구가 보냈는지 판단한다.
_SOURCES = (
    ("/v1/messages", "claude"),
    ("/v1/responses", "codex"),
    ("/chat/completions", "chat"),
)


def _get(obj, *path):
    for key in path:
        if obj is None:
            return None
        if isinstance(obj, dict):
            obj = obj.get(key)
        else:
            obj = getattr(obj, key, None)
    return obj


def _source(kwargs):
    url = str(_get(kwargs, "litellm_params", "proxy_server_request", "url") or "")
    for path, name in _SOURCES:
        if path in url:
            return name
    call_type = str(kwargs.get("call_type") or "")
    if "anthropic" in call_type:
        return "claude"
    if "responses" in call_type:
        return "codex"
    return "other"


def _usage(kwargs, response_obj):
    usage = _get(kwargs, "standard_logging_object") or {}
    out = {
        "prompt_tokens": usage.get("prompt_tokens"),
        "completion_tokens": usage.get("completion_tokens"),
        "reasoning_tokens": None,
    }
    u = _get(response_obj, "usage")
    if u is not None:
        out["prompt_tokens"] = out["prompt_tokens"] or _get(u, "prompt_tokens") or _get(u, "input_tokens")
        out["completion_tokens"] = out["completion_tokens"] or _get(u, "completion_tokens") or _get(u, "output_tokens")
        out["reasoning_tokens"] = (
            _get(u, "completion_tokens_details", "reasoning_tokens")
            or _get(u, "output_tokens_details", "reasoning_tokens")
            or _get(u, "reasoning_tokens")
        )
    return out


def _write(record):
    path = os.environ.get("EXAONE_USAGE_LOG")
    if not path:
        return
    line = json.dumps(record, ensure_ascii=False)
    with _LOCK:
        with open(path, "a", encoding="utf-8") as f:
            f.write(line + "\n")


def _seconds(start, end):
    try:
        return (end - start).total_seconds()
    except Exception:  # noqa: BLE001
        return None


class UsageLogger(CustomLogger):
    def log_pre_api_call(self, model, messages, kwargs):
        """업스트림을 부르기 직전마다 불린다 (재시도 포함)."""
        trace = kwargs.get("litellm_call_id")
        if not trace:
            return
        with _LOCK:
            entry = _ATTEMPTS.setdefault(trace, [0, time.time()])
            entry[0] += 1
            # 오래된 항목 정리 (끝 알림을 못 받은 요청)
            if len(_ATTEMPTS) > 1000:
                cutoff = time.time() - 3600
                for k in [k for k, v in _ATTEMPTS.items() if v[1] < cutoff]:
                    del _ATTEMPTS[k]

    async def async_log_success_event(self, kwargs, response_obj, start_time, end_time):
        self._log(kwargs, response_obj, start_time, end_time, ok=True)

    # async_log_failure_event 는 첫 시도가 실패할 때 한 번만 오고(그 뒤 게이트웨이가 재시도한다),
    # 최종 실패에는 오지 않는다. 최종 실패는 클라이언트에 오류를 돌려줄 때의 훅으로 기록한다.

    async def async_post_call_failure_hook(self, request_data, original_exception, user_api_key_dict, traceback_str=None):
        try:
            trace = request_data.get("litellm_call_id")
            route = str(_get(request_data, "metadata", "user_api_key_request_route") or _get(request_data, "proxy_server_request", "url") or "")
            source = next((name for path, name in _SOURCES if path in route), "other")
            now = time.time()
            with _LOCK:
                attempts, first = _ATTEMPTS.pop(trace, [None, None]) if trace else [None, None]
            _write(
                {
                    "ts": now,
                    "source": source,
                    "ok": False,
                    "status": getattr(original_exception, "status_code", None) or 500,
                    "seconds": round(now - first, 3) if first else None,
                    "attempts": attempts,
                    "error": str(original_exception)[:200],
                }
            )
        except Exception:  # noqa: BLE001
            pass
        return None

    def _log(self, kwargs, response_obj, start_time, end_time, ok):
        try:
            exc = kwargs.get("exception")
            status = 200 if ok else (getattr(exc, "status_code", None) or 500)
            now = time.time()
            with _LOCK:
                attempts, first = _ATTEMPTS.pop(kwargs.get("litellm_call_id"), [1, None])
            record = {
                "ts": now,
                "source": _source(kwargs),
                "ok": ok,
                "status": status,
                # 게이트웨이가 처음 시도한 때부터 (재시도 대기 포함)
                "seconds": round(now - first, 3) if first else _seconds(start_time, end_time),
                "attempts": attempts,
                **(_usage(kwargs, response_obj) if ok else {}),
            }
            if not ok and exc is not None:
                record["error"] = str(exc)[:200]
            _write(record)
        except Exception:  # noqa: BLE001  사용 기록 실패가 요청을 막으면 안 된다.
            pass


proxy_handler_instance = UsageLogger()
