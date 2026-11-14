import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { marked } from "marked";
import DOMPurify from "dompurify";
import { chatApi, Conversation, Effort, Message } from "./chatApi";

const EFFORT_KEY = "exaone.chatEffort";
const EFFORT_LABEL: Record<Effort, string> = { low: "짧게", high: "보통", max: "최대" };
const EFFORT_HELP =
  "추론 강도: EXAONE이 답하기 전에 생각하는 양입니다. 실제로는 차이가 크지 않고(추론 수십 토큰, 1~2초), 응답 속도는 서버 혼잡에 더 크게 좌우됩니다.";

const secs = (ms: number) => (ms < 10_000 ? (ms / 1000).toFixed(1) : Math.round(ms / 1000).toString());

function loadEffort(): Effort {
  try {
    const v = localStorage.getItem(EFFORT_KEY);
    return v === "low" || v === "max" ? v : "high";
  } catch {
    return "high";
  }
}

function Markdown({ text }: { text: string }) {
  const html = useMemo(() => DOMPurify.sanitize(marked.parse(text, { async: false, gfm: true, breaks: true }) as string), [text]);
  return <div className="md" dangerouslySetInnerHTML={{ __html: html }} />;
}

interface Live {
  reasoning: string;
  content: string;
  startedAt: number;
  retry: [number, number] | null;
}

export default function Chat({ visible, gatewayReady }: { visible: boolean; gatewayReady: boolean }) {
  const [convs, setConvs] = useState<Conversation[]>([]);
  const [active, setActive] = useState<number | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [live, setLive] = useState<Record<number, Live>>({});
  const [running, setRunning] = useState<Set<number>>(new Set());
  const [error, setError] = useState("");
  const activeRef = useRef(active);
  activeRef.current = active;

  const reloadList = useCallback(async () => setConvs(await chatApi.list()), []);

  useEffect(() => {
    reloadList();
    chatApi.running().then((ids) => setRunning(new Set(ids)));
    const offDelta = chatApi.onDelta((d) =>
      setLive((prev) => {
        const cur = prev[d.conversationId] ?? { reasoning: "", content: "", startedAt: Date.now(), retry: null };
        return {
          ...prev,
          [d.conversationId]: {
            ...cur,
            reasoning: cur.reasoning + (d.reasoning ?? ""),
            content: cur.content + (d.content ?? ""),
            retry: d.retry ?? (d.reasoning || d.content ? null : cur.retry),
          },
        };
      }),
    );
    const offDone = chatApi.onDone((d) => {
      setLive((prev) => {
        const next = { ...prev };
        delete next[d.conversationId];
        return next;
      });
      setRunning((prev) => {
        const next = new Set(prev);
        next.delete(d.conversationId);
        return next;
      });
      if (d.conversationId === activeRef.current) setMessages((m) => [...m, d.message]);
      reloadList();
    });
    return () => {
      offDelta();
      offDone();
    };
  }, [reloadList]);

  useEffect(() => {
    setError("");
    if (active === null) setMessages([]);
    else chatApi.messages(active).then(setMessages);
  }, [active]);

  const newChat = async () => {
    const c = await chatApi.create();
    await reloadList();
    setActive(c.id);
  };

  const send = async (text: string, effort: Effort) => {
    setError("");
    let id = active;
    if (id === null) {
      id = (await chatApi.create()).id;
      setActive(id);
    }
    try {
      setRunning((prev) => new Set(prev).add(id!));
      setLive((prev) => ({ ...prev, [id!]: { reasoning: "", content: "", startedAt: Date.now(), retry: null } }));
      const m = await chatApi.send(id, text, effort);
      setMessages((prev) => (activeRef.current === id ? [...prev, m] : prev));
      reloadList();
      return true;
    } catch (e) {
      setRunning((prev) => {
        const next = new Set(prev);
        next.delete(id!);
        return next;
      });
      setError(String(e));
      return false;
    }
  };

  const remove = async (id: number) => {
    await chatApi.remove(id);
    if (active === id) setActive(null);
    reloadList();
  };

  const rename = async (id: number) => {
    const cur = convs.find((c) => c.id === id);
    const title = window.prompt("대화 이름", cur?.title ?? "");
    if (title && title.trim()) {
      await chatApi.rename(id, title.trim());
      reloadList();
    }
  };

  const isRunning = active !== null && running.has(active);
  const current = active !== null ? live[active] : undefined;

  return (
    <div className="chat" style={{ display: visible ? "flex" : "none" }}>
      <aside className="chat-list">
        <button className="primary new-chat" onClick={newChat}>+ 새 대화</button>
        <div className="convs">
          {convs.map((c) => (
            <div
              key={c.id}
              className={`conv ${active === c.id ? "on" : ""}`}
              onClick={() => setActive(c.id)}
              onDoubleClick={() => rename(c.id)}
              title="두 번 클릭하면 이름을 바꿉니다"
            >
              {running.has(c.id) && <span className="dot starting" />}
              <span className="conv-title">{c.title}</span>
              <button
                className="tab-close"
                aria-label="대화 지우기"
                onClick={(e) => {
                  e.stopPropagation();
                  if (window.confirm(`"${c.title}" 대화를 지울까요? 되돌릴 수 없습니다.`)) remove(c.id);
                }}
              >
                ×
              </button>
            </div>
          ))}
          {convs.length === 0 && <p className="hint pad">아직 대화가 없습니다.</p>}
        </div>
      </aside>

      <section className="chat-main">
        <Thread messages={messages} live={current} running={isRunning} />
        {!gatewayReady && <div className="chat-warn">게이트웨이가 꺼져 있습니다. 대시보드에서 먼저 켜세요.</div>}
        {error && <div className="chat-warn error">{error}</div>}
        <Composer
          disabled={!gatewayReady}
          running={isRunning}
          onSend={send}
          onStop={() => active !== null && chatApi.stop(active)}
          focusKey={`${visible}-${active}-${isRunning}-${gatewayReady}`}
          visible={visible}
        />
      </section>
    </div>
  );
}

function Waiting({ live }: { live?: Live }) {
  const [, tick] = useState(0);
  useEffect(() => {
    const t = setInterval(() => tick((n) => n + 1), 1000);
    return () => clearInterval(t);
  }, []);
  const waited = live ? Math.floor((Date.now() - live.startedAt) / 1000) : 0;
  return (
    <div className="thinking">
      응답을 기다리는 중… {waited > 0 && `${waited}초`}
      {live?.retry && (
        <span className="retry">
          {" "}
          · 서버가 붐벼 다시 시도 중 ({live.retry[0]}/{live.retry[1]})
        </span>
      )}
    </div>
  );
}

function Thread({ messages, live, running }: { messages: Message[]; live?: Live; running: boolean }) {
  const box = useRef<HTMLDivElement>(null);
  const stick = useRef(true);

  useEffect(() => {
    const el = box.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [messages, live]);

  if (messages.length === 0 && !live) {
    return (
      <div className="thread empty">
        <div>
          <h1>EXAONE과 대화</h1>
          <p className="sub">K-EXAONE 2.5에게 바로 질문합니다. 대화 기록은 이 컴퓨터에만 저장됩니다.</p>
        </div>
      </div>
    );
  }

  return (
    <div
      className="thread"
      ref={box}
      onScroll={(e) => {
        const el = e.currentTarget;
        stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 60;
      }}
    >
      {messages.map((m) => (
        <Bubble key={m.id} m={m} />
      ))}
      {running && (
        <div className="msg assistant">
          {live?.reasoning ? (
            <details className="reasoning" open={!live.content}>
              <summary>{live.content ? "생각한 과정" : "생각하는 중…"}</summary>
              <div className="reasoning-text">{live.reasoning}</div>
            </details>
          ) : (
            !live?.content && <Waiting live={live} />
          )}
          {live?.content && <Markdown text={live.content.replace(/^\s+/, "")} />}
        </div>
      )}
    </div>
  );
}

function Bubble({ m }: { m: Message }) {
  if (m.role === "user") {
    return (
      <div className="msg user">
        <div className="user-text">{m.content}</div>
      </div>
    );
  }
  const gen = m.totalMs != null && m.firstTokenMs != null ? m.totalMs - m.firstTokenMs : null;
  const tokens = [
    m.firstTokenMs != null && `대기 ${secs(m.firstTokenMs)}초`,
    gen != null && `생성 ${secs(gen)}초`,
    m.completionTokens != null && `출력 ${m.completionTokens.toLocaleString()} 토큰`,
    m.reasoningTokens != null && `추론 ${m.reasoningTokens.toLocaleString()}`,
    m.retries ? `재시도 ${m.retries}회` : null,
  ].filter(Boolean);
  return (
    <div className="msg assistant">
      {m.reasoning && (
        <details className="reasoning">
          <summary>생각한 과정</summary>
          <div className="reasoning-text">{m.reasoning}</div>
        </details>
      )}
      {m.content && <Markdown text={m.content} />}
      {m.error && <div className="msg-error">{m.error}</div>}
      <div className="msg-meta">
        {tokens.join(" · ")}
        {m.content && (
          <button className="small ghost" onClick={() => navigator.clipboard.writeText(m.content)}>
            복사
          </button>
        )}
      </div>
    </div>
  );
}

function Composer({
  disabled,
  running,
  onSend,
  onStop,
  focusKey,
  visible,
}: {
  disabled: boolean;
  running: boolean;
  onSend: (text: string, effort: Effort) => Promise<boolean>;
  onStop: () => void;
  focusKey: string;
  visible: boolean;
}) {
  const [text, setText] = useState("");
  const [effort, setEffort] = useState<Effort>(loadEffort);
  const ta = useRef<HTMLTextAreaElement>(null);

  // 보이는 상태에서만 포커스한다 (숨겨진 요소는 포커스를 받지 못한다). 화면이 바뀐 직후에 옮긴다.
  useEffect(() => {
    if (!visible || disabled) return;
    const id = window.setTimeout(() => ta.current?.focus(), 0);
    return () => window.clearTimeout(id);
  }, [focusKey, visible, disabled]);

  // 내용에 맞춰 높이를 늘린다 (최대 8줄 정도).
  useEffect(() => {
    const el = ta.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 200)}px`;
  }, [text]);

  const submit = async () => {
    const t = text.trim();
    if (!t || running || disabled) return;
    setText("");
    if (!(await onSend(t, effort))) setText(t);
  };

  return (
    <div
      className={`composer ${disabled ? "off" : ""}`}
      // 입력창 바깥 여백을 눌러도 입력창으로 커서가 가게 한다 (버튼은 제외).
      onMouseDown={(e) => {
        const t = e.target as HTMLElement;
        if (t.closest("button") || t === ta.current) return;
        e.preventDefault();
        ta.current?.focus();
      }}
    >
      <textarea
        ref={ta}
        rows={1}
        value={text}
        placeholder={disabled ? "게이트웨이를 켜면 입력할 수 있습니다" : "메시지 입력 (Enter 보내기, Shift+Enter 줄바꿈)"}
        disabled={disabled}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          // 한글 조합 중 Enter 는 글자 확정이므로 보내지 않는다.
          if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing && e.keyCode !== 229) {
            e.preventDefault();
            submit();
          }
        }}
      />
      <div className="composer-bar">
        <div className="effort" title={EFFORT_HELP}>
        <span className="effort-label">추론</span>
        <div className="seg small-seg">
          {(["low", "high", "max"] as Effort[]).map((v) => (
            <button
              key={v}
              className={effort === v ? "on" : ""}
              onClick={() => {
                setEffort(v);
                try {
                  localStorage.setItem(EFFORT_KEY, v);
                } catch {
                  /* 무시 */
                }
              }}
            >
              {EFFORT_LABEL[v]}
            </button>
          ))}
        </div>
        </div>
        {running ? (
          <button onClick={onStop}>멈추기</button>
        ) : (
          <button className="primary" onClick={submit} disabled={disabled || !text.trim()}>
            보내기
          </button>
        )}
      </div>
    </div>
  );
}
