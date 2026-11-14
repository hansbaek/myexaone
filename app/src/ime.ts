// 한글 등 조합형 입력(IME) 처리.
//
// macOS WKWebView(Tauri)에서 확인한 한글 입력 방식 (tests/ime-webkit-korean.log):
//   - composition 이벤트가 오지 않는다.
//   - 새 글자의 첫 자모는 input(insertText "ㅎ") 으로 들어오고,
//     조합이 진행되면 input(insertReplacementText "하" → "한") 으로 직전 글자를 바꿔치기한다.
//   - 받침이 다음 글자로 넘어가면 insertReplacementText "하" 다음에 insertText "나" 가 온다.
//   - 글자 키의 keydown 은 input 보다 늦게, keyCode 229 로 온다.
//   - 띄어쓰기·Enter 등은 keydown(일반 keyCode) 이 먼저 오고 input 이 뒤따른다.
// xterm.js 는 insertText 의 첫 자모만 보내고 바꿔치기는 무시해서 "ㅎㄱㅇ" 처럼 자모만 남는다.
//
// 그래서 조합 중인 글자 하나(pending)를 앱이 들고 있다가 바꿔치기를 반영하고,
// 다음 글자가 시작되거나 다른 키가 눌리면 확정해서 보낸다.
// composition 이벤트를 쓰는 환경(다른 입력기)도 같은 방식으로 처리한다.
//
// Windows(WebView2 = Chromium)는 xterm.js 기본 처리를 쓴다.
import type { Terminal } from "@xterm/xterm";

const ua = navigator.userAgent;
const IS_WEBKIT = /AppleWebKit/.test(ua) && !/Chrome|Chromium|Edg\//.test(ua);
const MODE_KEY = "exaone.imeMode"; // "auto" | "custom" | "xterm"
const DEBUG_KEY = "exaone.imeDebug";

export type ImeMode = "auto" | "custom" | "xterm";

function read(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function write(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* 저장 못 해도 동작에는 지장 없음 */
  }
}

export function imeMode(): ImeMode {
  const v = read(MODE_KEY);
  return v === "custom" || v === "xterm" ? v : "auto";
}

export const setImeMode = (v: ImeMode) => write(MODE_KEY, v);
export const imeDebug = () => read(DEBUG_KEY) === "1";
export const setImeDebug = (on: boolean) => write(DEBUG_KEY, on ? "1" : "0");
export const usesCustomIme = () => imeMode() === "custom" || (imeMode() === "auto" && IS_WEBKIT);

/** 한글 자모·음절인지 (조합 중일 수 있는 글자) */
export function isHangul(s: string): boolean {
  return /^[ᄀ-ᇿ㄰-㆏가-힣]+$/.test(s);
}

type Log = (line: string) => void;

const MODIFIER_KEYS = new Set(["Shift", "Control", "Alt", "Meta", "CapsLock", "Fn", "FnLock", "AltGraph", "Hyper", "Super", "OS"]);

/** 조합 상태. DOM 과 분리해 두어 시험하기 쉽게 했다. */
export class HangulBuffer {
  pending = "";
  constructor(
    private readonly send: (data: string) => void,
    private readonly onPreview: (text: string) => void,
  ) {}

  private set(text: string) {
    this.pending = text;
    this.onPreview(text);
  }

  /** 조합 중인 글자를 확정해서 보낸다. */
  flush() {
    if (this.pending) {
      const p = this.pending;
      this.set("");
      this.send(p);
    }
  }

  /** input(insertText): 새 글자 시작. 한글이 아니면 바로 보낸다. */
  insertText(data: string) {
    this.flush();
    if (isHangul(data)) this.set(data);
    else this.send(data);
  }

  /** input(insertReplacementText): 조합 중인 글자를 바꾼다. */
  replace(data: string) {
    if (isHangul(data)) this.set(data);
    else this.insertText(data);
  }

  /** composition 이벤트 방식 */
  compositionUpdate(data: string) {
    this.set(data);
  }

  compositionEnd(data: string) {
    this.set("");
    if (data) this.send(data);
  }

  /** Backspace: 조합 중인 글자가 있으면 그것만 지운다 (아직 보내지 않았으므로). */
  backspace(): boolean {
    if (!this.pending) return false;
    this.set("");
    return true;
  }
}

/**
 * 터미널에 조합 입력 처리를 붙인다. 반환값은 해제 함수.
 * @param rawSend 세션으로 보내는 함수
 * @param log 진단 기록 (꺼져 있으면 undefined)
 */
export function installIme(term: Terminal, rawSend: (data: string) => void, log?: Log): () => void {
  const root = term.element;
  if (!root) return () => {};
  const send = (data: string) => {
    log?.(`→ send ${JSON.stringify(data)}`);
    rawSend(data);
  };
  const custom = usesCustomIme();
  log?.(`ime mode=${imeMode()} custom=${custom} webkit=${IS_WEBKIT} ua=${ua}`);

  const listeners: [string, EventListener][] = [];
  const on = (type: string, fn: (e: Event) => void) => {
    root.addEventListener(type, fn, true);
    listeners.push([type, fn]);
  };

  if (log) {
    for (const type of ["keydown", "keyup", "compositionstart", "compositionupdate", "compositionend", "beforeinput", "input"]) {
      on(type, (e) => {
        const k = e as KeyboardEvent & InputEvent & CompositionEvent;
        const ta = term.textarea;
        log(
          [
            type,
            "key" in k && k.key !== undefined ? `key=${JSON.stringify(k.key)} code=${k.code} keyCode=${k.keyCode}` : "",
            "isComposing" in k ? `isComposing=${k.isComposing}` : "",
            "inputType" in k && k.inputType ? `inputType=${k.inputType}` : "",
            "data" in k && k.data !== undefined ? `data=${JSON.stringify(k.data)}` : "",
            ta ? `ta=${JSON.stringify(ta.value.slice(-12))}` : "",
          ]
            .filter(Boolean)
            .join(" "),
        );
      });
    }
  }

  if (!custom) {
    return () => listeners.forEach(([t, fn]) => root.removeEventListener(t, fn, true));
  }

  const preview = document.createElement("div");
  preview.className = "ime-preview";
  root.querySelector(".xterm-screen")?.appendChild(preview);

  const placePreview = () => {
    const screen = root.querySelector(".xterm-screen") as HTMLElement | null;
    if (!screen) return;
    const cw = screen.clientWidth / term.cols;
    const ch = screen.clientHeight / term.rows;
    const buf = term.buffer.active;
    preview.style.left = `${buf.cursorX * cw}px`;
    preview.style.top = `${buf.cursorY * ch}px`;
    preview.style.height = `${ch}px`;
    preview.style.lineHeight = `${ch}px`;
  };

  const buf = new HangulBuffer(send, (text) => {
    preview.textContent = text;
    preview.classList.toggle("on", text.length > 0);
    if (text) placePreview();
  });

  // 마지막으로 xterm 에 넘긴 keydown 의 시각. 그 직후의 insertText 는 xterm 이 keypress 로 이미 보냈다.
  let xtermKeyAt = -1;
  let composing = false;

  const stop = (e: Event) => {
    e.stopImmediatePropagation();
    e.stopPropagation();
  };

  on("compositionstart", (e) => {
    stop(e);
    composing = true;
    buf.flush();
  });
  on("compositionupdate", (e) => {
    stop(e);
    buf.compositionUpdate((e as CompositionEvent).data ?? "");
  });
  on("compositionend", (e) => {
    stop(e);
    composing = false;
    buf.compositionEnd((e as CompositionEvent).data ?? buf.pending);
  });

  on("input", (e) => {
    const ie = e as InputEvent;
    const data = ie.data ?? "";
    switch (ie.inputType) {
      case "insertCompositionText":
      case "deleteCompositionText":
      case "insertFromComposition":
        stop(e);
        return;
      case "insertReplacementText":
        stop(e);
        if (data) buf.replace(data);
        return;
      case "insertText":
        stop(e);
        if (!data) return;
        // 띄어쓰기 등은 keydown 이 먼저 와서 xterm 이 이미 보냈다.
        if (!isHangul(data) && performance.now() - xtermKeyAt < 50) return;
        buf.insertText(data);
        return;
    }
  });

  term.attachCustomKeyEventHandler((ev) => {
    if (ev.type !== "keydown") return !(composing || ev.isComposing || ev.keyCode === 229);
    if (composing || ev.isComposing || ev.keyCode === 229) return false;
    // Shift 등 수정 키만 누른 것은 글자 입력의 일부다 (예: ㅆ = Shift+ㅅ). 조합을 확정하지 않는다.
    if (MODIFIER_KEYS.has(ev.key)) return true;
    if (ev.key === "Backspace" && buf.backspace()) return false;
    // 다른 키(띄어쓰기, Enter, 방향키, Ctrl+C 등)를 누르면 조합 중인 글자를 먼저 확정한다.
    buf.flush();
    xtermKeyAt = performance.now();
    return true;
  });

  // 포커스를 잃으면 조합 중인 글자를 확정한다.
  const blur = () => buf.flush();
  term.textarea?.addEventListener("blur", blur);

  return () => {
    listeners.forEach(([t, fn]) => root.removeEventListener(t, fn, true));
    term.textarea?.removeEventListener("blur", blur);
    preview.remove();
  };
}
