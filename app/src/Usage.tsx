import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface Totals {
  requests: number;
  failed: number;
  promptTokens: number;
  completionTokens: number;
  reasoningTokens: number;
  attempts: number;
  busy: number;
  avgSeconds: number | null;
}
interface SourceTotals extends Totals {
  source: string;
}
interface DayTotals {
  day: string;
  source: string;
  requests: number;
  failed: number;
  tokens: number;
}
interface Summary {
  total: Totals;
  bySource: SourceTotals[];
  byDay: DayTotals[];
  firstTs: number | null;
}

// 색은 출처를 따라간다 (순위가 아니라). 순서 고정.
const SOURCES = ["claude", "codex", "chat", "other"] as const;
const SOURCE_LABEL: Record<string, string> = { claude: "Claude Code", codex: "Codex", chat: "대화 창", other: "기타" };
const SOURCE_VAR: Record<string, string> = {
  claude: "var(--series-1)",
  codex: "var(--series-2)",
  chat: "var(--series-3)",
  other: "var(--series-4)",
};

type Range = 1 | 7 | 30;
const RANGE_LABEL: Record<Range, string> = { 1: "오늘", 7: "7일", 30: "30일" };

const inTauri = "__TAURI_INTERNALS__" in window;

/** 이 컴퓨터 시간대의 YYYY-MM-DD */
function ymd(d: Date) {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

/** 기간의 날짜 목록 (오늘 포함, 오래된 날부터) */
function days(range: Range): string[] {
  const out: string[] = [];
  const today = new Date();
  for (let i = range - 1; i >= 0; i--) {
    const d = new Date(today.getFullYear(), today.getMonth(), today.getDate() - i);
    out.push(ymd(d));
  }
  return out;
}

function since(range: Range): number {
  const t = new Date();
  return new Date(t.getFullYear(), t.getMonth(), t.getDate() - (range - 1)).getTime() / 1000;
}

function mockSummary(range: Range): Summary {
  const byDay: DayTotals[] = [];
  days(range).forEach((day, i) => {
    SOURCES.slice(0, 3).forEach((source, j) => {
      const tokens = ((i * 7 + j * 13) % 9) * 900 + (j === 0 ? 2400 : 300);
      byDay.push({ day, source, requests: Math.round(tokens / 400), failed: (i + j) % 4 === 0 ? 1 : 0, tokens });
    });
  });
  const sum = (src?: string): Totals => {
    const rows = byDay.filter((d) => !src || d.source === src);
    const tokens = rows.reduce((a, d) => a + d.tokens, 0);
    const requests = rows.reduce((a, d) => a + d.requests, 0);
    const failed = rows.reduce((a, d) => a + d.failed, 0);
    return {
      requests,
      failed,
      promptTokens: Math.round(tokens * 0.7),
      completionTokens: Math.round(tokens * 0.3),
      reasoningTokens: Math.round(tokens * 0.2),
      attempts: requests + failed * 6 + 3,
      busy: failed * 7 + 3,
      avgSeconds: 6.4,
    };
  };
  return {
    total: sum(),
    bySource: SOURCES.slice(0, 3).map((s) => ({ source: s, ...sum(s) })),
    byDay,
    firstTs: Date.now() / 1000 - 86400 * 3,
  };
}

const fmt = (n: number) => n.toLocaleString("ko-KR");
const compact = (n: number) =>
  n >= 1_000_000 ? `${(n / 1_000_000).toFixed(1)}M` : n >= 10_000 ? `${Math.round(n / 1000)}K` : n >= 1000 ? `${(n / 1000).toFixed(1)}K` : String(n);
const pct = (a: number, b: number) => (b > 0 ? `${((a / b) * 100).toFixed(a / b < 0.1 ? 1 : 0)}%` : "–");

export default function Usage({ visible }: { visible: boolean }) {
  const [range, setRange] = useState<Range>(7);
  const [data, setData] = useState<Summary | null>(null);
  const [error, setError] = useState("");

  const load = useCallback(async () => {
    try {
      setData(inTauri ? await invoke<Summary>("usage_summary", { since: since(range) }) : mockSummary(range));
      setError("");
    } catch (e) {
      setError(String(e));
    }
  }, [range]);

  // 보이는 동안 10초마다 새로 읽는다.
  useEffect(() => {
    if (!visible) return;
    load();
    const t = setInterval(load, 10_000);
    return () => clearInterval(t);
  }, [visible, load]);

  if (!visible) return null;

  const t = data?.total;
  return (
    <main className="usage">
      <div className="usage-head">
        <h1>사용량</h1>
        <div className="seg" role="group" aria-label="기간">
          {([1, 7, 30] as Range[]).map((r) => (
            <button key={r} className={range === r ? "on" : ""} onClick={() => setRange(r)}>
              {RANGE_LABEL[r]}
            </button>
          ))}
        </div>
      </div>
      {error && <p className="error">{error}</p>}
      {data && t && (
        <>
          <section className="tiles">
            <Tile label="요청" value={fmt(t.requests)} sub={t.failed ? `실패 ${fmt(t.failed)}` : "실패 없음"} />
            <Tile
              label="토큰"
              value={compact(t.promptTokens + t.completionTokens)}
              sub={`입력 ${compact(t.promptTokens)} · 출력 ${compact(t.completionTokens)} (추론 ${compact(t.reasoningTokens)})`}
            />
            <Tile
              label="서버 혼잡 (429)"
              value={pct(t.busy, t.attempts)}
              sub={`EXAONE 호출 ${fmt(t.attempts)}번 중 ${fmt(t.busy)}번`}
            />
            <Tile label="평균 응답 시간" value={t.avgSeconds != null ? `${t.avgSeconds.toFixed(1)}초` : "–"} sub="재시도 대기 포함" />
          </section>

          <h2>날짜별 토큰</h2>
          <section className="card">
            <DayChart data={data.byDay} range={range} />
          </section>

          <h2>출처별</h2>
          <section className="card table-card">
            <table className="usage-table">
              <thead>
                <tr>
                  <th>출처</th>
                  <th>요청</th>
                  <th>실패</th>
                  <th>입력 토큰</th>
                  <th>출력 토큰</th>
                  <th>추론</th>
                  <th>평균 시간</th>
                  <th>혼잡</th>
                </tr>
              </thead>
              <tbody>
                {data.bySource.length === 0 && (
                  <tr>
                    <td colSpan={8} className="muted">이 기간에는 기록이 없습니다.</td>
                  </tr>
                )}
                {[...data.bySource]
                  .sort((a, b) => SOURCES.indexOf(a.source as never) - SOURCES.indexOf(b.source as never))
                  .map((s) => (
                    <tr key={s.source}>
                      <td>
                        <span className="swatch" style={{ background: SOURCE_VAR[s.source] ?? SOURCE_VAR.other }} />
                        {SOURCE_LABEL[s.source] ?? s.source}
                      </td>
                      <td>{fmt(s.requests)}</td>
                      <td>{s.failed ? fmt(s.failed) : "–"}</td>
                      <td>{fmt(s.promptTokens)}</td>
                      <td>{fmt(s.completionTokens)}</td>
                      <td>{fmt(s.reasoningTokens)}</td>
                      <td>{s.avgSeconds != null ? `${s.avgSeconds.toFixed(1)}초` : "–"}</td>
                      <td>{pct(s.busy, s.attempts)}</td>
                    </tr>
                  ))}
              </tbody>
            </table>
          </section>
          <p className="hint">
            게이트웨이를 거친 요청만 셉니다 (Claude Code·Codex 세션, 대화 창). 질문·답변 내용은 기록하지 않습니다.
            {data.firstTs && ` 기록 시작: ${new Date(data.firstTs * 1000).toLocaleDateString("ko-KR")}`}
          </p>
        </>
      )}
    </main>
  );
}

function Tile({ label, value, sub }: { label: string; value: string; sub: string }) {
  return (
    <div className="tile">
      <div className="tile-label">{label}</div>
      <div className="tile-value">{value}</div>
      <div className="tile-sub">{sub}</div>
    </div>
  );
}

function DayChart({ data, range }: { data: DayTotals[]; range: Range }) {
  const [hover, setHover] = useState<{ day: string; x: number; y: number } | null>(null);
  const list = days(range);
  const present = SOURCES.filter((s) => data.some((d) => (d.source === s || (s === "other" && !SOURCES.includes(d.source as never))) && d.tokens > 0));

  const byDay = useMemo(() => {
    const m = new Map<string, Record<string, number>>();
    for (const d of data) {
      const src = SOURCES.includes(d.source as never) ? d.source : "other";
      const row = m.get(d.day) ?? {};
      row[src] = (row[src] ?? 0) + d.tokens;
      m.set(d.day, row);
    }
    return m;
  }, [data]);

  const max = Math.max(1, ...list.map((d) => Object.values(byDay.get(d) ?? {}).reduce((a, b) => a + b, 0)));
  // 보기 좋은 눈금 최댓값
  const step = 10 ** Math.floor(Math.log10(max));
  const top = Math.ceil(max / step) * step;

  const W = 640;
  const H = 200;
  const padL = 44;
  const padB = 22;
  const plotW = W - padL - 8;
  const plotH = H - padB - 8;
  const slot = plotW / list.length;
  const barW = Math.min(36, slot * 0.6);
  const ticks = [0, top / 2, top];

  if (present.length === 0) return <p className="muted pad">이 기간에는 기록이 없습니다.</p>;

  const hovered = hover ? byDay.get(hover.day) ?? {} : null;

  return (
    <div className="chart-wrap" onMouseLeave={() => setHover(null)}>
      {present.length >= 2 && (
        <div className="legend">
          {present.map((s) => (
            <span key={s} className="legend-item">
              <span className="swatch" style={{ background: SOURCE_VAR[s] }} />
              {SOURCE_LABEL[s]}
            </span>
          ))}
        </div>
      )}
      <svg viewBox={`0 0 ${W} ${H}`} className="chart" role="img" aria-label="날짜별 토큰 사용량 (막대: 출처별로 쌓음)">
        {ticks.map((v) => {
          const y = 8 + plotH - (v / top) * plotH;
          return (
            <g key={v}>
              <line x1={padL} x2={W - 8} y1={y} y2={y} className="grid" />
              <text x={padL - 6} y={y + 4} className="axis" textAnchor="end">
                {compact(v)}
              </text>
            </g>
          );
        })}
        {list.map((day, i) => {
          const row = byDay.get(day) ?? {};
          const cx = padL + slot * i + slot / 2;
          let y = 8 + plotH;
          const segs = SOURCES.filter((s) => row[s]).map((s) => {
            const h = (row[s] / top) * plotH;
            y -= h;
            return { s, y, h };
          });
          const showLabel = range <= 7 || i % 5 === 0 || i === list.length - 1;
          return (
            <g key={day}>
              {/* 막대보다 넓은 마우스 영역 */}
              <rect
                x={padL + slot * i}
                y={8}
                width={slot}
                height={plotH}
                fill="transparent"
                onMouseEnter={(e) => setHover({ day, x: e.clientX, y: e.clientY })}
                onMouseMove={(e) => setHover({ day, x: e.clientX, y: e.clientY })}
              />
              {segs.map(({ s, y: sy, h }, k) => {
                const isTop = k === segs.length - 1;
                // 위쪽 끝만 둥글게, 조각 사이 2px 간격
                const gap = k > 0 ? 2 : 0;
                const hh = Math.max(0, h - gap);
                const r = isTop ? Math.min(4, hh / 2, barW / 2) : 0;
                const x0 = cx - barW / 2;
                const d = `M${x0},${sy + hh} V${sy + r} Q${x0},${sy} ${x0 + r},${sy} H${x0 + barW - r} Q${x0 + barW},${sy} ${x0 + barW},${sy + r} V${sy + hh} Z`;
                return <path key={s} d={d} fill={SOURCE_VAR[s]} pointerEvents="none" />;
              })}
              {showLabel && (
                <text x={cx} y={H - 6} className="axis" textAnchor="middle">
                  {range === 1 ? "오늘" : `${Number(day.slice(5, 7))}/${Number(day.slice(8))}`}
                </text>
              )}
            </g>
          );
        })}
      </svg>
      {hover && hovered && (
        <div className="tooltip" style={{ left: hover.x + 12, top: hover.y + 12 }}>
          <div className="tooltip-title">{hover.day}</div>
          {SOURCES.filter((s) => hovered[s]).map((s) => (
            <div key={s} className="tooltip-row">
              <span className="swatch" style={{ background: SOURCE_VAR[s] }} />
              <span>{SOURCE_LABEL[s]}</span>
              <span className="tooltip-num">{fmt(hovered[s])}</span>
            </div>
          ))}
          {SOURCES.every((s) => !hovered[s]) && <div className="muted">기록 없음</div>}
        </div>
      )}
    </div>
  );
}
