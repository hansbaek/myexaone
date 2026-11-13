import { useCallback, useEffect, useRef, useState } from "react";
import { api, Check, LogLine, Phase, Status } from "./api";
import "./App.css";

type Tab = "home" | "logs" | "settings";

const PHASE_LABEL: Record<Phase, string> = {
  stopped: "꺼짐",
  starting: "시작하는 중",
  running: "실행 중",
  external: "실행 중 (외부)",
  error: "오류",
};

const MAX_LOGS = 2000;

export default function App() {
  const [tab, setTab] = useState<Tab>("home");
  const [status, setStatus] = useState<Status | null>(null);
  const [logs, setLogs] = useState<LogLine[]>([]);
  const lastSeq = useRef(0);

  const refresh = useCallback(async () => {
    setStatus(await api.status());
    const fresh = await api.logs(lastSeq.current);
    if (fresh.length) {
      lastSeq.current = fresh[fresh.length - 1].seq;
      setLogs((prev) => [...prev, ...fresh].slice(-MAX_LOGS));
    }
  }, []);

  useEffect(() => {
    refresh();
    const id = setInterval(refresh, 1500);
    return () => clearInterval(id);
  }, [refresh]);

  if (!status) return <main className="loading">불러오는 중…</main>;

  return (
    <div className="app">
      <nav className="side">
        <div className="brand">EXAONE Studio</div>
        <button className={tab === "home" ? "on" : ""} onClick={() => setTab("home")}>대시보드</button>
        <button className={tab === "logs" ? "on" : ""} onClick={() => setTab("logs")}>로그</button>
        <button className={tab === "settings" ? "on" : ""} onClick={() => setTab("settings")}>설정</button>
        <div className="side-foot">
          <span className={`dot ${status.phase}`} />
          {PHASE_LABEL[status.phase]}
        </div>
      </nav>
      <main>
        {tab === "home" && <Home status={status} refresh={refresh} goSettings={() => setTab("settings")} />}
        {tab === "logs" && <Logs logs={logs} />}
        {tab === "settings" && <Settings status={status} refresh={refresh} />}
      </main>
    </div>
  );
}

function Home({ status, refresh, goSettings }: { status: Status; refresh: () => Promise<void>; goSettings: () => void }) {
  const [checks, setChecks] = useState<Check[] | null>(null);
  const [checking, setChecking] = useState(false);
  const on = status.phase === "running" || status.phase === "starting";

  const toggle = async () => {
    await (on ? api.stop() : api.start());
    await refresh();
  };
  const diagnose = async () => {
    setChecking(true);
    try {
      setChecks(await api.doctor());
    } finally {
      setChecking(false);
    }
  };

  return (
    <>
      <h1>게이트웨이</h1>
      <section className="card hero">
        <div>
          <div className="phase">
            <span className={`dot big ${status.phase}`} />
            {PHASE_LABEL[status.phase]}
          </div>
          <div className="sub">
            {status.message ||
              (status.phase === "running" || status.phase === "external"
                ? status.url
                : "Claude Code와 Codex가 EXAONE을 쓰려면 게이트웨이가 켜져 있어야 합니다.")}
          </div>
        </div>
        {status.phase === "external" ? (
          <span className="hint">이 앱이 띄운 것이 아니라 여기서 끌 수 없습니다</span>
        ) : (
          <button className={on ? "" : "primary"} onClick={toggle} disabled={!status.apiKey && !on}>
            {status.phase === "starting" ? "취소" : on ? "끄기" : "켜기"}
          </button>
        )}
      </section>

      {!status.apiKey && (
        <section className="card notice">
          <span>API 키가 없습니다. 키를 입력해야 게이트웨이를 켤 수 있습니다.</span>
          <button className="primary" onClick={goSettings}>키 입력</button>
        </section>
      )}
      {!status.runtimeInstalled && status.apiKey && status.phase === "stopped" && (
        <section className="card notice">
          <span>처음 켤 때 실행 환경(LiteLLM, 약 500MB)을 설치합니다. 1~2분 걸립니다.</span>
        </section>
      )}

      <h2>터미널에서 쓰기</h2>
      <section className="card">
        <p className="sub">게이트웨이가 켜져 있는 동안, 작업할 폴더의 터미널에서 실행합니다.</p>
        <Copyable text="exaone claude" />
        <Copyable text="exaone codex" />
      </section>

      <h2>
        진단
        <button className="small" onClick={diagnose} disabled={checking}>{checking ? "확인 중…" : "실행"}</button>
      </h2>
      {checks && (
        <section className="card checks">
          {checks.map((c) => (
            <div key={c.name} className="check">
              <span className={`mark ${c.level}`}>{c.level === "ok" ? "✓" : c.level === "warn" ? "!" : "✗"}</span>
              <span className="check-name">{c.name}</span>
              <span className="check-detail">{c.detail}</span>
            </div>
          ))}
        </section>
      )}
    </>
  );
}

function Copyable({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    await navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 1200);
  };
  return (
    <div className="copyable">
      <code>{text}</code>
      <button className="small" onClick={copy}>{copied ? "복사됨" : "복사"}</button>
    </div>
  );
}

function Logs({ logs }: { logs: LogLine[] }) {
  const box = useRef<HTMLPreElement>(null);
  const stick = useRef(true);

  useEffect(() => {
    const el = box.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [logs]);

  return (
    <>
      <h1>로그</h1>
      <pre
        className="logs"
        ref={box}
        onScroll={(e) => {
          const el = e.currentTarget;
          stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
        }}
      >
        {logs.length ? logs.map((l) => l.text).join("\n") : "아직 로그가 없습니다. 게이트웨이를 켜면 여기에 표시됩니다."}
      </pre>
    </>
  );
}

function Settings({ status, refresh }: { status: Status; refresh: () => Promise<void> }) {
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const run = async (fn: () => Promise<void>) => {
    setBusy(true);
    setError("");
    try {
      await fn();
      setKey("");
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <h1>설정</h1>
      <h2>EXAONE API 키</h2>
      <section className="card">
        <p className="sub">
          {status.apiKey ? <>저장된 키: <code>{status.apiKey}</code></> : "저장된 키가 없습니다."} 키는 이 컴퓨터의
          키 저장소(macOS 키체인, Windows 자격 증명 관리자)에만 보관됩니다.
        </p>
        <form
          className="row"
          onSubmit={(e) => {
            e.preventDefault();
            if (key.trim()) run(() => api.setKey(key.trim()));
          }}
        >
          <input
            type="password"
            placeholder={status.apiKey ? "새 키로 바꾸려면 입력" : "API 키 붙여넣기"}
            value={key}
            onChange={(e) => setKey(e.target.value)}
            autoComplete="off"
            spellCheck={false}
          />
          <button className="primary" type="submit" disabled={busy || !key.trim()}>저장</button>
          {status.apiKey && (
            <button type="button" disabled={busy} onClick={() => run(api.deleteKey)}>삭제</button>
          )}
        </form>
        {error && <p className="error">{error}</p>}
        <p className="hint">키를 바꾼 뒤에는 게이트웨이를 껐다 켜야 새 키가 적용됩니다.</p>
      </section>

      <h2>정보</h2>
      <section className="card">
        <div className="kv"><span>게이트웨이 주소</span><code>{status.url}</code></div>
        <div className="kv"><span>실행 환경</span><span>{status.runtimeInstalled ? "설치됨" : "아직 설치되지 않음"}</span></div>
        <div className="kv"><span>버전</span><span>{status.version}</span></div>
      </section>
    </>
  );
}
