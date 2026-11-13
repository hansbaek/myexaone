// 한글 등 조합형 입력(IME) 처리.
//
// xterm.js 는 조합이 끝나면 숨은 textarea 값의 위치를 계산해 글자를 꺼내는데, macOS WebKit(Tauri가
// 쓰는 WKWebView)은 한글 조합 이벤트 순서와 textarea 갱신 시점이 Chromium과 달라서 글자가 빠진다
// (특히 받침이 다음 글자로 넘어가는 모음 입력). 그래서 WebKit 에서는 조합 이벤트를 xterm 보다 먼저
// 가로채서, 확정된 글자(compositionend 의 data)를 직접 보낸다.
//
// Windows(WebView2 = Chromium)는 xterm.js 의 기본 처리를 그대로 쓴다.
import type { Terminal } from "@xterm/xterm";

const ua = navigator.userAgent;
const IS_WEBKIT = /AppleWebKit/.test(ua) && !/Chrome|Chromium|Edg\//.test(ua);
const MODE_KEY = "exaone.imeMode"; // "auto" | "custom" | "xterm"
const DEBUG_KEY = "exaone.imeDebug";

export function imeMode(): "auto" | "custom" | "xterm" {
  try {
    const v = localStorage.getItem(MODE_KEY);
    return v === "custom" || v === "xterm" ? v : "auto";
  } catch {
    return "auto";
  }
}

export function setImeMode(v: "auto" | "custom" | "xterm") {
  try {
    localStorage.setItem(MODE_KEY, v);
  } catch {
    /* 무시 */
  }
}

export function imeDebug(): boolean {
  try {
    return localStorage.getItem(DEBUG_KEY) === "1";
  } catch {
    return false;
  }
}

export function setImeDebug(on: boolean) {
  try {
    localStorage.setItem(DEBUG_KEY, on ? "1" : "0");
  } catch {
    /* 무시 */
  }
}

export const usesCustomIme = () => imeMode() === "custom" || (imeMode() === "auto" && IS_WEBKIT);

type Log = (line: string) => void;

/** Enter 등 조합을 끝내는 특수 키가 keyCode 229 로만 올 때 보낼 값 */
const KEY_TO_DATA: Record<string, string> = {
  Enter: "\r",
  Backspace: "\x7f",
  Tab: "\t",
  Escape: "\x1b",
};

/**
 * 터미널에 조합 입력 처리를 붙인다. 반환값은 해제 함수.
 * @param send 확정된 글자를 세션으로 보내는 함수
 * @param log 진단 기록 (꺼져 있으면 undefined)
 */
export function installIme(term: Terminal, rawSend: (data: string) => void, log?: Log): () => void {
  const send = (data: string) => {
    log?.(`→ send ${JSON.stringify(data)}`);
    rawSend(data);
  };
  const root = term.element;
  if (!root) return () => {};
  const custom = usesCustomIme();
  log?.(`ime mode=${imeMode()} custom=${custom} webkit=${IS_WEBKIT} ua=${ua}`);

  const listeners: [string, EventListener][] = [];
  const on = (type: string, fn: (e: Event) => void) => {
    root.addEventListener(type, fn, true);
    listeners.push([type, fn]);
  };

  // 진단 기록은 처리 방식과 상관없이 모든 관련 이벤트를 남긴다.
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

  let composing = false;
  let lastUpdate = "";
  let endedAt = 0; // 마지막 compositionend 시각
  let sentKeyAt = 0; // keyCode 229 로 온 특수 키를 직접 보낸 시각
  let blockedKeydown = false; // 직전 keydown 을 xterm 이 처리하지 않게 막았는지

  const preview = document.createElement("div");
  preview.className = "ime-preview";
  term.element?.querySelector(".xterm-screen")?.appendChild(preview);

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
    // IME 후보 창이 커서 옆에 뜨도록 textarea 도 같은 위치로 옮긴다.
    const ta = term.textarea;
    if (ta) {
      ta.style.left = preview.style.left;
      ta.style.top = preview.style.top;
    }
  };

  const stop = (e: Event) => {
    e.stopImmediatePropagation();
    e.stopPropagation();
  };

  on("compositionstart", (e) => {
    stop(e);
    composing = true;
    lastUpdate = "";
    placePreview();
  });

  on("compositionupdate", (e) => {
    stop(e);
    lastUpdate = (e as CompositionEvent).data ?? "";
    preview.textContent = lastUpdate;
    preview.classList.toggle("on", lastUpdate.length > 0);
    placePreview();
  });

  on("compositionend", (e) => {
    stop(e);
    composing = false;
    const data = (e as CompositionEvent).data || lastUpdate;
    lastUpdate = "";
    preview.textContent = "";
    preview.classList.remove("on");
    endedAt = performance.now();
    if (data) send(data);
  });

  // 조합 중 textarea 에 들어가는 글자는 xterm 이 보내지 않게 막는다.
  // 조합 없이 바로 들어온 글자(한글 입력기 상태의 숫자·기호 등)는 xterm 이 무시하므로 직접 보낸다.
  on("input", (e) => {
    const ie = e as InputEvent;
    if (ie.inputType === "insertCompositionText" || ie.inputType === "deleteCompositionText" || ie.inputType === "insertFromComposition") {
      stop(e);
      return;
    }
    if (ie.inputType === "insertText" && ie.data && blockedKeydown && !composing) {
      stop(e);
      blockedKeydown = false;
      send(ie.data);
    }
  });

  term.attachCustomKeyEventHandler((ev) => {
    if (ev.type !== "keydown") return !(composing || ev.isComposing || ev.keyCode === 229);
    blockedKeydown = false;
    if (composing || ev.isComposing) {
      blockedKeydown = true;
      return false;
    }
    if (ev.keyCode === 229) {
      // 조합을 끝낸 직후 Enter 등이 229 로만 오면, 그 키를 직접 보낸다.
      const data = KEY_TO_DATA[ev.key];
      if (data && performance.now() - endedAt < 100) {
        sentKeyAt = performance.now();
        send(data);
      }
      blockedKeydown = true;
      return false;
    }
    // 같은 키가 229 와 일반 keyCode 로 두 번 오면 한 번만 보낸다.
    if (KEY_TO_DATA[ev.key] && performance.now() - sentKeyAt < 100) {
      return false;
    }
    return true;
  });

  return () => {
    listeners.forEach(([t, fn]) => root.removeEventListener(t, fn, true));
    preview.remove();
  };
}
