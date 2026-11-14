// 대화 창 통신. Tauri 밖(브라우저 미리보기)에서는 메모리 안의 가짜 대화로 동작한다.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type Effort = "low" | "high" | "max";

export interface Conversation {
  id: number;
  title: string;
  createdAt: number;
  updatedAt: number;
}

export interface Message {
  id: number;
  conversationId: number;
  role: "user" | "assistant";
  content: string;
  reasoning: string | null;
  promptTokens: number | null;
  completionTokens: number | null;
  reasoningTokens: number | null;
  error: string | null;
  firstTokenMs: number | null;
  totalMs: number | null;
  retries: number | null;
  createdAt: number;
}

export interface Delta {
  conversationId: number;
  reasoning: string | null;
  content: string | null;
  /** 서버 혼잡으로 다시 시도 중: [몇 번째, 최대] */
  retry: [number, number] | null;
}

export interface Done {
  conversationId: number;
  message: Message;
  title: string | null;
}

const inTauri = "__TAURI_INTERNALS__" in window;

// ── 브라우저 미리보기용 가짜 구현 ─────────────────────────────
const mock = {
  convs: [] as Conversation[],
  msgs: [] as Message[],
  id: 0,
  running: new Set<number>(),
  deltaFns: [] as ((d: Delta) => void)[],
  doneFns: [] as ((d: Done) => void)[],
};

function mockReply(conversationId: number, question: string) {
  const reasoning = `사용자가 "${question}"라고 물었다. 간단히 답한다.`;
  const answer = `**미리보기 응답**입니다.\n\n- 질문: ${question}\n- 실제 앱에서는 EXAONE이 답합니다.\n\n\`\`\`python\nprint("안녕, EXAONE")\n\`\`\``;
  const started = Date.now();
  const parts: Delta[] = [
    { conversationId, reasoning: null, content: null, retry: [1, 3] },
    ...reasoning.match(/.{1,6}/g)!.map((r) => ({ conversationId, reasoning: r, content: null, retry: null })),
    ...answer.match(/[\s\S]{1,5}/g)!.map((c) => ({ conversationId, reasoning: null, content: c, retry: null })),
  ];
  let i = 0;
  const step = () => {
    if (i === 0) {
      // 첫 조각(재시도 알림) 다음에는 서버 대기를 흉내 내느라 잠깐 쉰다.
      mock.deltaFns.forEach((f) => f(parts[0]));
      i = 1;
      clearInterval(t);
      setTimeout(() => mockReply2(), 1200);
      return;
    }
    if (!mock.running.has(conversationId) || i >= parts.length) {
      clearInterval(t);
      const msg: Message = {
        id: ++mock.id,
        conversationId,
        role: "assistant",
        content: i >= parts.length ? answer : parts.slice(0, i).map((p) => p.content ?? "").join(""),
        reasoning,
        promptTokens: 20,
        completionTokens: 60,
        reasoningTokens: 15,
        error: i >= parts.length ? null : "중간에 멈췄습니다",
        firstTokenMs: 1200,
        totalMs: Date.now() - started,
        retries: 1,
        createdAt: Date.now(),
      };
      mock.msgs.push(msg);
      mock.running.delete(conversationId);
      const conv = mock.convs.find((c) => c.id === conversationId)!;
      const title = conv.title === "새 대화" ? question.slice(0, 40) : null;
      if (title) conv.title = title;
      mock.doneFns.forEach((f) => f({ conversationId, message: msg, title }));
      return;
    }
    mock.deltaFns.forEach((f) => f(parts[i]));
    i++;
  };
  let t = setInterval(step, 25);
  const mockReply2 = () => (t = setInterval(step, 25));
}

async function mockInvoke(cmd: string, a: Record<string, unknown> = {}): Promise<unknown> {
  switch (cmd) {
    case "chat_list":
      return [...mock.convs].sort((x, y) => y.updatedAt - x.updatedAt);
    case "chat_messages":
      return mock.msgs.filter((m) => m.conversationId === a.id);
    case "chat_create": {
      const c = { id: ++mock.id, title: "새 대화", createdAt: Date.now(), updatedAt: Date.now() };
      mock.convs.push(c);
      return c;
    }
    case "chat_rename":
      mock.convs.find((c) => c.id === a.id)!.title = String(a.title);
      return;
    case "chat_delete":
      mock.convs = mock.convs.filter((c) => c.id !== a.id);
      mock.msgs = mock.msgs.filter((m) => m.conversationId !== a.id);
      return;
    case "chat_stop":
      mock.running.delete(a.id as number);
      return;
    case "chat_running":
      return [...mock.running];
    case "chat_send": {
      const id = a.id as number;
      const m: Message = {
        id: ++mock.id,
        conversationId: id,
        role: "user",
        content: String(a.text),
        reasoning: null,
        promptTokens: null,
        completionTokens: null,
        reasoningTokens: null,
        error: null,
        firstTokenMs: null,
        totalMs: null,
        retries: null,
        createdAt: Date.now(),
      };
      mock.msgs.push(m);
      mock.convs.find((c) => c.id === id)!.updatedAt = Date.now();
      mock.running.add(id);
      mockReply(id, String(a.text));
      return m;
    }
  }
}

function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return (inTauri ? invoke(cmd, args) : mockInvoke(cmd, args)) as Promise<T>;
}

export const chatApi = {
  list: () => call<Conversation[]>("chat_list"),
  messages: (id: number) => call<Message[]>("chat_messages", { id }),
  create: () => call<Conversation>("chat_create"),
  rename: (id: number, title: string) => call<void>("chat_rename", { id, title }),
  remove: (id: number) => call<void>("chat_delete", { id }),
  stop: (id: number) => call<void>("chat_stop", { id }),
  running: () => call<number[]>("chat_running"),
  send: (id: number, text: string, effort: Effort) => call<Message>("chat_send", { id, text, effort }),
  onDelta(fn: (d: Delta) => void): () => void {
    if (!inTauri) {
      mock.deltaFns.push(fn);
      return () => (mock.deltaFns = mock.deltaFns.filter((f) => f !== fn));
    }
    const p = listen<Delta>("chat-delta", (e) => fn(e.payload));
    return () => void p.then((u) => u());
  },
  onDone(fn: (d: Done) => void): () => void {
    if (!inTauri) {
      mock.doneFns.push(fn);
      return () => (mock.doneFns = mock.doneFns.filter((f) => f !== fn));
    }
    const p = listen<Done>("chat-done", (e) => fn(e.payload));
    return () => void p.then((u) => u());
  },
};
