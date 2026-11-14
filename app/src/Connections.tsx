import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface McpStatus {
  installed: boolean;
  registered: boolean;
  command: string[] | null;
  upToDate: boolean;
}
interface McpInfo {
  command: string[];
  snippet: string;
  claude: McpStatus;
  codex: McpStatus;
}
type Tool = "claude" | "codex";

const inTauri = "__TAURI_INTERNALS__" in window;

// ── 브라우저 미리보기용 가짜 구현 ─────────────────────────────
const mockCmd = ["/Applications/EXAONE Studio.app/Contents/MacOS/exaone-studio", "mcp"];
const mock: Record<Tool, McpStatus> = {
  claude: { installed: true, registered: false, command: null, upToDate: false },
  codex: { installed: true, registered: true, command: ["/old/place/exaone-studio", "mcp"], upToDate: false },
};
async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (inTauri) return invoke<T>(cmd, args);
  await new Promise((r) => setTimeout(r, 400));
  const tool = args?.tool as Tool;
  if (cmd === "mcp_register") mock[tool] = { installed: true, registered: true, command: mockCmd, upToDate: true };
  if (cmd === "mcp_unregister") mock[tool] = { installed: true, registered: false, command: null, upToDate: false };
  return {
    command: mockCmd,
    snippet: JSON.stringify({ mcpServers: { exaone: { command: mockCmd[0], args: ["mcp"] } } }, null, 2),
    claude: { ...mock.claude },
    codex: { ...mock.codex },
  } as T;
}

const TOOL_LABEL: Record<Tool, string> = { claude: "Claude Code", codex: "Codex" };

export default function Connections({ visible }: { visible: boolean }) {
  const [info, setInfo] = useState<McpInfo | null>(null);
  const [busy, setBusy] = useState<Tool | null>(null);
  const [error, setError] = useState("");

  const load = useCallback(async () => {
    try {
      setInfo(await call<McpInfo>("mcp_info"));
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    if (visible) load();
  }, [visible, load]);

  if (!visible) return null;

  const act = async (tool: Tool, register: boolean) => {
    setBusy(tool);
    setError("");
    try {
      await call(register ? "mcp_register" : "mcp_unregister", { tool });
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <main className="connections">
      <h1>도구 연결</h1>
      <p className="sub intro">
        평소처럼 Claude Code나 Codex를 쓰면서, 필요할 때 EXAONE에게 질문을 맡길 수 있게 <code>exaone_chat</code> 도구(MCP)를
        등록합니다. 게이트웨이가 꺼져 있어도 동작합니다. EXAONE을 메인 모델로 쓰려면 세션 탭을 쓰세요.
      </p>
      {error && <p className="error">{error}</p>}
      {!info && !error && <p className="muted">확인하는 중…</p>}
      {info && (
        <>
          {(["claude", "codex"] as Tool[]).map((tool) => (
            <ToolCard key={tool} tool={tool} st={info[tool]} busy={busy === tool} onAct={(r) => act(tool, r)} />
          ))}

          <h2>그 밖의 MCP 클라이언트 (Antigravity 등)</h2>
          <section className="card">
            <p className="sub">설정 파일(MCP 서버 설정)에 아래 내용을 추가하세요.</p>
            <Snippet text={info.snippet} />
            <p className="hint">앱을 다른 폴더로 옮기면 경로가 바뀌므로 다시 등록해야 합니다.</p>
          </section>
        </>
      )}
    </main>
  );
}

function ToolCard({ tool, st, busy, onAct }: { tool: Tool; st: McpStatus; busy: boolean; onAct: (register: boolean) => void }) {
  let state: { level: "ok" | "warn" | "off"; text: string };
  if (!st.installed) state = { level: "off", text: `${TOOL_LABEL[tool]}가 설치돼 있지 않습니다` };
  else if (!st.registered) state = { level: "off", text: "등록 안 됨" };
  else if (!st.upToDate) state = { level: "warn", text: "다른 위치의 앱으로 등록돼 있습니다. 다시 등록하세요." };
  else state = { level: "ok", text: "등록됨 · 모든 프로젝트에서 사용 가능" };

  return (
    <section className="card tool-card">
      <div className="tool-main">
        <div className="tool-name">{TOOL_LABEL[tool]}</div>
        <div className={`tool-state ${state.level}`}>
          <span className="mark">{state.level === "ok" ? "✓" : state.level === "warn" ? "!" : "–"}</span>
          {state.text}
        </div>
        {st.registered && st.command && !st.upToDate && (
          <div className="hint mono" title={st.command.join(" ")}>
            현재: {st.command.join(" ")}
          </div>
        )}
      </div>
      {st.installed && (
        <div className="tool-actions">
          {(!st.registered || !st.upToDate) && (
            <button className="primary" disabled={busy} onClick={() => onAct(true)}>
              {busy ? "처리 중…" : st.registered ? "다시 등록" : "등록"}
            </button>
          )}
          {st.registered && (
            <button disabled={busy} onClick={() => onAct(false)}>
              해제
            </button>
          )}
        </div>
      )}
    </section>
  );
}

function Snippet({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <div className="snippet">
      <pre>{text}</pre>
      <button
        className="small"
        onClick={async () => {
          await navigator.clipboard.writeText(text);
          setCopied(true);
          setTimeout(() => setCopied(false), 1200);
        }}
      >
        {copied ? "복사됨" : "복사"}
      </button>
    </div>
  );
}
