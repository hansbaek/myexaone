import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { attach, sessions, ToolName } from "./sessionsApi";
import { imeDebug, installIme } from "./ime";

interface Tab {
  key: number;
  tool: ToolName;
  cwd: string;
  exited?: number;
}

const RECENT_KEY = "exaone.recentFolders";
const TOOL_LABEL: Record<ToolName, string> = { claude: "Claude Code", codex: "Codex" };

function loadRecent(): string[] {
  try {
    return JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]");
  } catch {
    return [];
  }
}

function saveRecent(list: string[]) {
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify(list));
  } catch {
    /* 저장 못 해도 동작에는 지장 없음 */
  }
}

const baseName = (p: string) => p.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || p;

export default function Sessions({ visible, gatewayReady }: { visible: boolean; gatewayReady: boolean }) {
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [active, setActive] = useState<number | null>(null); // null = 새 세션 화면
  const nextKey = useRef(1);

  const open = (tool: ToolName, cwd: string) => {
    const key = nextKey.current++;
    setTabs((t) => [...t, { key, tool, cwd }]);
    setActive(key);
    const recent = [cwd, ...loadRecent().filter((r) => r !== cwd)].slice(0, 8);
    saveRecent(recent);
  };

  const close = (key: number) => {
    setTabs((t) => {
      const rest = t.filter((x) => x.key !== key);
      if (active === key) setActive(rest.length ? rest[rest.length - 1].key : null);
      return rest;
    });
  };

  const markExited = (key: number, code: number) =>
    setTabs((t) => t.map((x) => (x.key === key ? { ...x, exited: code } : x)));

  return (
    <div className="sessions" style={{ display: visible ? "flex" : "none" }}>
      <div className="tabbar">
        {tabs.map((t) => (
          <div key={t.key} className={`tab ${active === t.key ? "on" : ""}`} onClick={() => setActive(t.key)}>
            <span className={`dot ${t.exited === undefined ? "running" : "stopped"}`} />
            <span className="tab-title" title={t.cwd}>
              {TOOL_LABEL[t.tool]} · {baseName(t.cwd)}
            </span>
            <button
              className="tab-close"
              aria-label="세션 닫기"
              onClick={(e) => {
                e.stopPropagation();
                close(t.key);
              }}
            >
              ×
            </button>
          </div>
        ))}
        <button className={`tab new ${active === null ? "on" : ""}`} onClick={() => setActive(null)}>
          + 새 세션
        </button>
      </div>

      <div className="term-area">
        {active === null && <NewSession onOpen={open} gatewayReady={gatewayReady} />}
        {tabs.map((t) => (
          <TermView
            key={t.key}
            tab={t}
            visible={visible && active === t.key}
            onExit={(code) => markExited(t.key, code)}
          />
        ))}
      </div>
    </div>
  );
}

function NewSession({ onOpen, gatewayReady }: { onOpen: (tool: ToolName, cwd: string) => void; gatewayReady: boolean }) {
  const [tool, setTool] = useState<ToolName>("claude");
  const [cwd, setCwd] = useState("");
  const [recent, setRecent] = useState<string[]>(loadRecent);

  useEffect(() => {
    if (!cwd) {
      const r = loadRecent();
      if (r[0]) setCwd(r[0]);
      else sessions.homeDir().then((h) => h && setCwd(h));
    }
  }, [cwd]);

  const pick = async () => {
    const dir = await sessions.pickFolder();
    if (dir) setCwd(dir);
  };

  return (
    <div className="new-session">
      <h1>새 세션</h1>
      {!gatewayReady && (
        <section className="card notice">
          <span>게이트웨이가 꺼져 있습니다. 대시보드에서 먼저 켜세요.</span>
        </section>
      )}
      <h2>도구</h2>
      <div className="seg">
        {(["claude", "codex"] as ToolName[]).map((t) => (
          <button key={t} className={tool === t ? "on" : ""} onClick={() => setTool(t)}>
            {TOOL_LABEL[t]}
          </button>
        ))}
      </div>
      <h2>작업 폴더</h2>
      <div className="row">
        <input value={cwd} onChange={(e) => setCwd(e.target.value)} spellCheck={false} placeholder="폴더 경로" />
        <button onClick={pick}>찾아보기…</button>
      </div>
      {recent.length > 0 && (
        <div className="recent">
          {recent.map((r) => (
            <div key={r} className="recent-item">
              <button className="link" onClick={() => setCwd(r)} title={r}>
                {r}
              </button>
              <button
                className="small"
                aria-label="최근 목록에서 지우기"
                onClick={() => {
                  const next = recent.filter((x) => x !== r);
                  saveRecent(next);
                  setRecent(next);
                }}
              >
                ×
              </button>
            </div>
          ))}
        </div>
      )}
      <div className="actions">
        <button className="primary" disabled={!cwd.trim() || !gatewayReady} onClick={() => onOpen(tool, cwd.trim())}>
          {TOOL_LABEL[tool]} 시작
        </button>
      </div>
    </div>
  );
}

function cssVar(name: string) {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

function TermView({ tab, visible, onExit }: { tab: Tab; visible: boolean; onExit: (code: number) => void }) {
  const host = useRef<HTMLDivElement>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const termRef = useRef<Terminal | null>(null);
  const idRef = useRef<number | null>(null);
  const onExitRef = useRef(onExit);
  onExitRef.current = onExit;

  useEffect(() => {
    const term = new Terminal({
      fontFamily: 'ui-monospace, "SF Mono", Menlo, Consolas, "D2Coding", monospace',
      fontSize: 13,
      cursorBlink: true,
      allowProposedApi: true,
      scrollback: 5000,
      theme: { background: cssVar("--term-bg"), foreground: cssVar("--term-fg"), cursor: cssVar("--term-fg") },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host.current!);
    fit.fit();
    termRef.current = term;
    fitRef.current = fit;

    let detach = () => {};
    let disposed = false;
    sessions
      .open(tab.tool, tab.cwd, term.cols, term.rows)
      .then((id) => {
        if (disposed) {
          sessions.close(id);
          return;
        }
        idRef.current = id;
        detach = attach(id, {
          output: (d) => term.write(d),
          exit: (code) => {
            term.write(`\r\n\x1b[2m[종료됨 · 코드 ${code}]\x1b[0m\r\n`);
            onExitRef.current(code);
          },
        });
      })
      .catch((e) => {
        term.write(`\x1b[31m시작하지 못했습니다: ${String(e)}\x1b[0m\r\n`);
        onExitRef.current(1);
      });

    const send = (d: string) => idRef.current !== null && sessions.write(idRef.current, d);
    const removeIme = installIme(term, send, imeDebug() ? (line) => sessions.imeLog(line) : undefined);
    const input = term.onData(send);
    const resize = term.onResize(({ cols, rows }) => idRef.current !== null && sessions.resize(idRef.current, cols, rows));
    const ro = new ResizeObserver(() => {
      if (host.current && host.current.offsetWidth > 0) fit.fit();
    });
    ro.observe(host.current!);

    return () => {
      disposed = true;
      ro.disconnect();
      removeIme();
      input.dispose();
      resize.dispose();
      detach();
      if (idRef.current !== null) sessions.close(idRef.current);
      term.dispose();
    };
  }, [tab.tool, tab.cwd]);

  useEffect(() => {
    if (visible) {
      requestAnimationFrame(() => {
        fitRef.current?.fit();
        termRef.current?.focus();
      });
    }
  }, [visible]);

  return <div className="term" ref={host} style={{ display: visible ? "block" : "none" }} />;
}
