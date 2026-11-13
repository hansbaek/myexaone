// 터미널 세션 통신. 출력 이벤트는 세션 id 가 정해지기 전에 올 수도 있어 id 별로 모아 두었다가 넘긴다.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type ToolName = "claude" | "codex";

type Handlers = { output: (data: string) => void; exit: (code: number) => void };

const inTauri = "__TAURI_INTERNALS__" in window;
const handlers = new Map<number, Handlers>();
const pending = new Map<number, { data: string[]; exit?: number }>();

function deliverOutput(id: number, data: string) {
  const h = handlers.get(id);
  if (h) h.output(data);
  else {
    const p = pending.get(id) ?? { data: [] };
    p.data.push(data);
    pending.set(id, p);
  }
}

function deliverExit(id: number, code: number) {
  const h = handlers.get(id);
  if (h) h.exit(code);
  else {
    const p = pending.get(id) ?? { data: [] };
    p.exit = code;
    pending.set(id, p);
  }
}

if (inTauri) {
  listen<{ id: number; data: string }>("session-output", (e) => deliverOutput(e.payload.id, e.payload.data));
  listen<{ id: number; code: number }>("session-exit", (e) => deliverExit(e.payload.id, e.payload.code));
}

/** 세션 출력을 받기 시작한다. 그사이 쌓인 출력도 바로 넘긴다. */
export function attach(id: number, h: Handlers): () => void {
  handlers.set(id, h);
  const p = pending.get(id);
  if (p) {
    pending.delete(id);
    p.data.forEach(h.output);
    if (p.exit !== undefined) h.exit(p.exit);
  }
  return () => handlers.delete(id);
}

// ── 브라우저 미리보기: 입력을 그대로 되돌려 주는 가짜 터미널 ─────────
let mockId = 0;
const mockIntro = (tool: ToolName, cwd: string) =>
  `\x1b[1m${tool === "claude" ? "Claude Code" : "Codex"}\x1b[0m (미리보기) · k-exaone · ${cwd}\r\n\r\n> `;

export const sessions = {
  open: async (tool: ToolName, cwd: string, cols: number, rows: number): Promise<number> => {
    if (inTauri) return invoke<number>("open_session", { tool, cwd, cols, rows });
    const id = ++mockId;
    setTimeout(() => deliverOutput(id, mockIntro(tool, cwd)), 200);
    return id;
  },
  write: (id: number, data: string) => {
    if (inTauri) return invoke<void>("write_session", { id, data });
    deliverOutput(id, data === "\r" ? "\r\n> " : data);
    return Promise.resolve();
  },
  resize: (id: number, cols: number, rows: number) =>
    inTauri ? invoke<void>("resize_session", { id, cols, rows }) : Promise.resolve(),
  close: (id: number) => (inTauri ? invoke<void>("close_session", { id }) : Promise.resolve()),
  imeLog: (line: string) => (inTauri ? invoke<void>("ime_log", { line }) : (console.debug("[ime]", line), Promise.resolve())),
  imeLogPath: () => (inTauri ? invoke<string>("ime_log_path") : Promise.resolve("(미리보기)")),
  homeDir: () => (inTauri ? invoke<string | null>("home_dir") : Promise.resolve("/Users/me")),
  pickFolder: async (): Promise<string | null> => {
    if (!inTauri) return "/Users/me/projects/demo";
    const { open } = await import("@tauri-apps/plugin-dialog");
    const r = await open({ directory: true, multiple: false, title: "작업할 폴더 선택" });
    return typeof r === "string" ? r : null;
  },
};
