// 앱의 Rust 쪽 명령 호출. Tauri 밖(브라우저에서 화면만 볼 때)에서는 가짜 데이터로 동작한다.
import { invoke } from "@tauri-apps/api/core";

export type Phase = "stopped" | "starting" | "running" | "external" | "error";

export interface Status {
  phase: Phase;
  message: string;
  url: string;
  port: number;
  apiKey: string | null;
  runtimeInstalled: boolean;
  version: string;
}

export interface LogLine {
  seq: number;
  text: string;
}

export interface Check {
  name: string;
  level: "ok" | "warn" | "fail";
  detail: string;
}

const inTauri = "__TAURI_INTERNALS__" in window;

// ── 브라우저 미리보기용 가짜 구현 ─────────────────────────────
const mock = {
  phase: "stopped" as Phase,
  key: "sk-V… (46자)" as string | null,
  seq: 0,
  logs: [] as LogLine[],
  log(text: string) {
    this.logs.push({ seq: ++this.seq, text });
  },
};

async function mockInvoke(cmd: string, args?: Record<string, unknown>): Promise<unknown> {
  switch (cmd) {
    case "get_status":
      return {
        phase: mock.phase,
        message: mock.phase === "starting" ? "게이트웨이 시작 중…" : "",
        url: "http://127.0.0.1:4000",
        port: 4000,
        apiKey: mock.key,
        runtimeInstalled: true,
        version: "0.1.0",
      } satisfies Status;
    case "start_gateway":
      if (!mock.key) {
        mock.phase = "error";
        return;
      }
      mock.phase = "starting";
      mock.log("게이트웨이 시작 중…");
      setTimeout(() => {
        mock.phase = "running";
        mock.log("INFO:     Uvicorn running on http://127.0.0.1:4000");
        mock.log("게이트웨이 준비 완료: http://127.0.0.1:4000");
      }, 1500);
      return;
    case "stop_gateway":
      mock.phase = "stopped";
      mock.log("게이트웨이를 껐습니다");
      return;
    case "get_logs":
      return mock.logs.filter((l) => l.seq > Number(args?.after ?? 0));
    case "set_api_key":
      mock.key = `${String(args?.key).slice(0, 4)}… (${String(args?.key).length}자)`;
      return;
    case "delete_api_key":
      mock.key = null;
      return;
    case "run_doctor":
      return [
        { name: "uv", level: "ok", detail: "/Applications/EXAONE Studio.app/Contents/MacOS/uv" },
        { name: "실행 환경", level: "ok", detail: "~/Library/Application Support/EXAONE Studio/runtime/venv" },
        { name: "API 키", level: mock.key ? "ok" : "fail", detail: mock.key ?? "없음" },
        { name: "게이트웨이", level: mock.phase === "running" ? "ok" : "warn", detail: mock.phase === "running" ? "http://127.0.0.1:4000 에서 실행 중" : "꺼져 있음" },
        { name: "Claude Code", level: "ok", detail: "/opt/homebrew/bin/claude" },
        { name: "Codex", level: "warn", detail: "`codex` 명령을 찾을 수 없음" },
      ] satisfies Check[];
  }
}

function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return (inTauri ? invoke(cmd, args) : mockInvoke(cmd, args)) as Promise<T>;
}

export const api = {
  status: () => call<Status>("get_status"),
  start: () => call<void>("start_gateway"),
  stop: () => call<void>("stop_gateway"),
  logs: (after: number) => call<LogLine[]>("get_logs", { after }),
  setKey: (key: string) => call<void>("set_api_key", { key }),
  deleteKey: () => call<void>("delete_api_key"),
  doctor: () => call<Check[]>("run_doctor"),
};
