//! 사용량 통계. 게이트웨이가 요청마다 남기는 기록(logs/usage.jsonl, gateway/usage_logger.py)을
//! 저장소(studio.db)로 모아 출처별·날짜별로 집계한다. 질문·답변 내용은 다루지 않는다.

use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use anyhow::Result;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::paths;
use crate::store::Store;

pub fn log_file() -> Result<PathBuf> {
    Ok(paths::logs_dir()?.join("usage.jsonl"))
}

pub(crate) fn init(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS usage_events (
            id INTEGER PRIMARY KEY,
            ts REAL NOT NULL,
            source TEXT NOT NULL,
            ok INTEGER NOT NULL,
            status INTEGER,
            seconds REAL,
            attempts INTEGER,
            prompt_tokens INTEGER,
            completion_tokens INTEGER,
            reasoning_tokens INTEGER
        );
        CREATE INDEX IF NOT EXISTS usage_by_ts ON usage_events(ts);
        CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    )?;
    Ok(())
}

/// usage_logger.py 가 남기는 한 줄.
#[derive(Debug, Deserialize)]
struct Record {
    ts: f64,
    source: String,
    ok: bool,
    status: Option<i64>,
    seconds: Option<f64>,
    attempts: Option<i64>,
    prompt_tokens: Option<i64>,
    completion_tokens: Option<i64>,
    reasoning_tokens: Option<i64>,
}

const OFFSET_KEY: &str = "usage_log_offset";

/// 기록 파일에서 새로 생긴 줄을 저장소로 옮긴다. 옮긴 줄 수를 돌려준다.
/// 파일이 처음부터 다시 쓰였으면(크기가 줄었으면) 처음부터 읽는다.
pub fn ingest(store: &Store, path: &Path) -> Result<usize> {
    let conn = &store.conn;
    let Ok(file) = std::fs::File::open(path) else {
        return Ok(0);
    };
    let len = file.metadata()?.len();
    let mut offset: u64 = conn
        .query_row(
            "SELECT value FROM meta WHERE key = ?1",
            params![OFFSET_KEY],
            |r| r.get::<_, String>(0),
        )
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if offset > len {
        offset = 0;
    }
    let mut reader = BufReader::new(file);
    reader.seek(SeekFrom::Start(offset))?;

    let tx = conn.unchecked_transaction()?;
    let mut count = 0;
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        // 아직 쓰는 중인 마지막 줄(줄바꿈 없음)은 다음에 읽는다.
        if n == 0 || !line.ends_with('\n') {
            break;
        }
        offset += n as u64;
        let Ok(r) = serde_json::from_str::<Record>(line.trim()) else {
            continue;
        };
        tx.execute(
            "INSERT INTO usage_events (ts, source, ok, status, seconds, attempts, prompt_tokens, completion_tokens, reasoning_tokens)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                r.ts,
                r.source,
                r.ok,
                r.status,
                r.seconds,
                r.attempts,
                r.prompt_tokens,
                r.completion_tokens,
                r.reasoning_tokens
            ],
        )?;
        count += 1;
    }
    tx.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![OFFSET_KEY, offset.to_string()],
    )?;
    tx.commit()?;
    Ok(count)
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub requests: i64,
    pub failed: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub reasoning_tokens: i64,
    /// 업스트림 호출 횟수 (게이트웨이 안의 재시도 포함)
    pub attempts: i64,
    /// 그중 서버 혼잡(429)으로 실패한 호출
    pub busy: i64,
    /// 성공한 요청의 평균 응답 시간(초, 재시도 대기 포함)
    pub avg_seconds: Option<f64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SourceTotals {
    pub source: String,
    #[serde(flatten)]
    pub totals: Totals,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DayTotals {
    /// 이 컴퓨터 시간대 기준 YYYY-MM-DD
    pub day: String,
    pub source: String,
    pub requests: i64,
    pub failed: i64,
    pub tokens: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub total: Totals,
    pub by_source: Vec<SourceTotals>,
    pub by_day: Vec<DayTotals>,
    /// 기록이 시작된 시각 (없으면 null)
    pub first_ts: Option<f64>,
}

/// 실패한 요청은 모든 시도가, 성공한 요청은 마지막을 뺀 시도가 혼잡(429)으로 실패한 것으로 본다.
const AGG: &str = "COUNT(*),
     COALESCE(SUM(CASE WHEN ok THEN 0 ELSE 1 END), 0),
     COALESCE(SUM(prompt_tokens), 0),
     COALESCE(SUM(completion_tokens), 0),
     COALESCE(SUM(reasoning_tokens), 0),
     COALESCE(SUM(COALESCE(attempts, 1)), 0),
     COALESCE(SUM(CASE WHEN ok THEN COALESCE(attempts, 1) - 1
                       WHEN status = 429 THEN COALESCE(attempts, 1)
                       ELSE COALESCE(attempts, 1) - 1 END), 0),
     AVG(CASE WHEN ok THEN seconds END)";

fn totals(r: &rusqlite::Row, at: usize) -> rusqlite::Result<Totals> {
    Ok(Totals {
        requests: r.get(at)?,
        failed: r.get(at + 1)?,
        prompt_tokens: r.get(at + 2)?,
        completion_tokens: r.get(at + 3)?,
        reasoning_tokens: r.get(at + 4)?,
        attempts: r.get(at + 5)?,
        busy: r.get(at + 6)?,
        avg_seconds: r.get(at + 7)?,
    })
}

/// `since`(유닉스 초) 이후의 사용량을 집계한다.
pub fn summary(store: &Store, since: f64) -> Result<Summary> {
    let conn = &store.conn;
    let total = conn.query_row(
        &format!("SELECT {AGG} FROM usage_events WHERE ts >= ?1"),
        params![since],
        |r| totals(r, 0),
    )?;
    let mut st = conn.prepare(&format!(
        "SELECT source, {AGG} FROM usage_events WHERE ts >= ?1 GROUP BY source ORDER BY COUNT(*) DESC"
    ))?;
    let by_source = st
        .query_map(params![since], |r| {
            Ok(SourceTotals {
                source: r.get(0)?,
                totals: totals(r, 1)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut st = conn.prepare(
        "SELECT date(ts, 'unixepoch', 'localtime') AS day, source, COUNT(*),
                COALESCE(SUM(CASE WHEN ok THEN 0 ELSE 1 END), 0),
                COALESCE(SUM(COALESCE(prompt_tokens, 0) + COALESCE(completion_tokens, 0)), 0)
         FROM usage_events WHERE ts >= ?1 GROUP BY day, source ORDER BY day, source",
    )?;
    let by_day = st
        .query_map(params![since], |r| {
            Ok(DayTotals {
                day: r.get(0)?,
                source: r.get(1)?,
                requests: r.get(2)?,
                failed: r.get(3)?,
                tokens: r.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let first_ts = conn.query_row("SELECT MIN(ts) FROM usage_events", [], |r| r.get(0))?;
    Ok(Summary {
        total,
        by_source,
        by_day,
        first_ts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn line(ts: f64, source: &str, ok: bool, status: i64, attempts: i64, p: i64, c: i64) -> String {
        format!(
            r#"{{"ts": {ts}, "source": "{source}", "ok": {ok}, "status": {status}, "seconds": 2.0, "attempts": {attempts}, "prompt_tokens": {p}, "completion_tokens": {c}, "reasoning_tokens": 1}}"#
        ) + "\n"
    }

    #[test]
    fn ingests_incrementally_and_summarizes() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("usage.jsonl");
        let store = Store::in_memory().unwrap();
        let now = 1_800_000_000.0;
        {
            let mut f = std::fs::File::create(&log).unwrap();
            f.write_all(line(now, "claude", true, 200, 3, 10, 5).as_bytes())
                .unwrap();
            f.write_all(line(now + 1.0, "codex", true, 200, 1, 20, 7).as_bytes())
                .unwrap();
            // 아직 쓰는 중인 줄 (줄바꿈 없음)은 이번에 읽지 않는다
            f.write_all(br#"{"ts": 1800000002, "source": "chat""#)
                .unwrap();
        }
        assert_eq!(ingest(&store, &log).unwrap(), 2);
        assert_eq!(
            ingest(&store, &log).unwrap(),
            0,
            "같은 줄을 두 번 옮기지 않는다"
        );
        {
            let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
            f.write_all(b", \"ok\": false, \"status\": 429, \"seconds\": 37.0, \"attempts\": 7}\n")
                .unwrap();
            f.write_all(b"not json\n").unwrap();
        }
        assert_eq!(
            ingest(&store, &log).unwrap(),
            1,
            "이어 쓴 줄을 읽고, 깨진 줄은 건너뛴다"
        );

        let s = summary(&store, 0.0).unwrap();
        assert_eq!(s.total.requests, 3);
        assert_eq!(s.total.failed, 1);
        assert_eq!(s.total.prompt_tokens, 30);
        assert_eq!(s.total.attempts, 3 + 1 + 7);
        // claude 2번(성공 전 재시도) + chat 7번(모두 429)
        assert_eq!(s.total.busy, 9);
        assert_eq!(
            s.total.avg_seconds,
            Some(2.0),
            "실패한 요청은 평균 시간에서 뺀다"
        );
        assert_eq!(s.by_source.len(), 3);
        let claude = s.by_source.iter().find(|x| x.source == "claude").unwrap();
        assert_eq!(claude.totals.busy, 2);
        assert_eq!(s.by_day.iter().map(|d| d.tokens).sum::<i64>(), 15 + 27);
        assert_eq!(s.first_ts, Some(now));

        // 기간 밖은 빠진다
        assert_eq!(summary(&store, now + 0.5).unwrap().total.requests, 2);
    }

    #[test]
    fn restarts_when_log_is_recreated() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("usage.jsonl");
        let store = Store::in_memory().unwrap();
        std::fs::write(&log, line(1.0, "chat", true, 200, 1, 1, 1).repeat(3)).unwrap();
        assert_eq!(ingest(&store, &log).unwrap(), 3);
        std::fs::write(&log, line(2.0, "chat", true, 200, 1, 1, 1)).unwrap();
        assert_eq!(ingest(&store, &log).unwrap(), 1);
        assert_eq!(ingest(&store, &dir.path().join("none.jsonl")).unwrap(), 0);
    }
}
