//! 로컬 저장소 (SQLite, 데이터 폴더의 studio.db). 대화 기록을 둔다. 사용량 통계도 여기에 더할 예정.

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::paths;

pub struct Store {
    conn: Connection,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: i64,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: i64,
    pub conversation_id: i64,
    /// user | assistant
    pub role: String,
    pub content: String,
    pub reasoning: Option<String>,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub error: Option<String>,
    pub created_at: i64,
}

/// 답변 하나를 저장할 때 넘기는 값.
#[derive(Debug, Clone, Default)]
pub struct AssistantReply {
    pub content: String,
    pub reasoning: Option<String>,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub error: Option<String>,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS conversations (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY,
    conversation_id INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    reasoning TEXT,
    prompt_tokens INTEGER,
    completion_tokens INTEGER,
    reasoning_tokens INTEGER,
    error TEXT,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS messages_by_conversation ON messages(conversation_id, id);
";

impl Store {
    /// 데이터 폴더의 저장소를 연다 (없으면 만든다).
    pub fn open_default() -> Result<Self> {
        let dir = paths::data_dir()?;
        std::fs::create_dir_all(&dir)?;
        Self::open(&dir.join("studio.db"))
    }

    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("저장소를 열 수 없습니다: {}", path.display()))?;
        Self::init(conn)
    }

    pub fn in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    pub fn create_conversation(&self, title: &str) -> Result<Conversation> {
        let t = now();
        self.conn.execute(
            "INSERT INTO conversations (title, created_at, updated_at) VALUES (?1, ?2, ?2)",
            params![title, t],
        )?;
        Ok(Conversation {
            id: self.conn.last_insert_rowid(),
            title: title.into(),
            created_at: t,
            updated_at: t,
        })
    }

    /// 최근에 쓴 순서.
    pub fn conversations(&self) -> Result<Vec<Conversation>> {
        let mut st = self
            .conn
            .prepare("SELECT id, title, created_at, updated_at FROM conversations ORDER BY updated_at DESC, id DESC")?;
        let rows = st.query_map([], |r| {
            Ok(Conversation {
                id: r.get(0)?,
                title: r.get(1)?,
                created_at: r.get(2)?,
                updated_at: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn rename_conversation(&self, id: i64, title: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE conversations SET title = ?2 WHERE id = ?1",
            params![id, title],
        )?;
        Ok(())
    }

    pub fn delete_conversation(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM conversations WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn messages(&self, conversation_id: i64) -> Result<Vec<Message>> {
        let mut st = self.conn.prepare(
            "SELECT id, conversation_id, role, content, reasoning, prompt_tokens, completion_tokens,
                    reasoning_tokens, error, created_at
             FROM messages WHERE conversation_id = ?1 ORDER BY id",
        )?;
        let rows = st.query_map(params![conversation_id], |r| {
            Ok(Message {
                id: r.get(0)?,
                conversation_id: r.get(1)?,
                role: r.get(2)?,
                content: r.get(3)?,
                reasoning: r.get(4)?,
                prompt_tokens: r.get(5)?,
                completion_tokens: r.get(6)?,
                reasoning_tokens: r.get(7)?,
                error: r.get(8)?,
                created_at: r.get(9)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// 최근 사용 순서가 같은 밀리초에도 뒤섞이지 않도록, 항상 지금까지의 최댓값보다 크게 둔다.
    fn touch(&self, conversation_id: i64, t: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE conversations
             SET updated_at = MAX(?2, (SELECT COALESCE(MAX(updated_at), 0) + 1 FROM conversations))
             WHERE id = ?1",
            params![conversation_id, t],
        )?;
        Ok(())
    }

    pub fn add_user_message(&self, conversation_id: i64, content: &str) -> Result<Message> {
        let t = now();
        self.conn.execute(
            "INSERT INTO messages (conversation_id, role, content, created_at) VALUES (?1, 'user', ?2, ?3)",
            params![conversation_id, content, t],
        )?;
        let id = self.conn.last_insert_rowid();
        self.touch(conversation_id, t)?;
        self.message(id)
    }

    pub fn add_assistant_message(
        &self,
        conversation_id: i64,
        reply: &AssistantReply,
    ) -> Result<Message> {
        let t = now();
        self.conn.execute(
            "INSERT INTO messages (conversation_id, role, content, reasoning, prompt_tokens, completion_tokens,
                                   reasoning_tokens, error, created_at)
             VALUES (?1, 'assistant', ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                conversation_id,
                reply.content,
                reply.reasoning,
                reply.prompt_tokens,
                reply.completion_tokens,
                reply.reasoning_tokens,
                reply.error,
                t
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        self.touch(conversation_id, t)?;
        self.message(id)
    }

    fn message(&self, id: i64) -> Result<Message> {
        let conv: i64 = self.conn.query_row(
            "SELECT conversation_id FROM messages WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )?;
        self.messages(conv)?
            .into_iter()
            .find(|m| m.id == id)
            .context("방금 저장한 메시지를 찾을 수 없습니다")
    }

    /// 마지막 메시지(답변 다시 받기 전에 지울 때 쓴다).
    pub fn delete_message(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM messages WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn conversation(&self, id: i64) -> Result<Option<Conversation>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, title, created_at, updated_at FROM conversations WHERE id = ?1",
                params![id],
                |r| {
                    Ok(Conversation {
                        id: r.get(0)?,
                        title: r.get(1)?,
                        created_at: r.get(2)?,
                        updated_at: r.get(3)?,
                    })
                },
            )
            .optional()?)
    }
}

/// 첫 질문으로 대화 제목을 만든다 (한 줄, 최대 40자).
pub fn title_from(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("새 대화");
    let mut t: String = line.chars().take(40).collect();
    if line.chars().count() > 40 {
        t.push('…');
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversation_roundtrip_and_cascade() {
        let s = Store::in_memory().unwrap();
        let c = s.create_conversation("첫 대화").unwrap();
        s.add_user_message(c.id, "안녕").unwrap();
        let a = s
            .add_assistant_message(
                c.id,
                &AssistantReply {
                    content: "반가워요".into(),
                    reasoning: Some("인사".into()),
                    completion_tokens: Some(5),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(a.role, "assistant");
        assert_eq!(a.reasoning.as_deref(), Some("인사"));
        let msgs = s.messages(c.id).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].content, "안녕");

        let c2 = s.create_conversation("둘째").unwrap();
        // 최근에 쓴 대화가 먼저
        s.add_user_message(c.id, "다시").unwrap();
        let list = s.conversations().unwrap();
        assert_eq!(list[0].id, c.id);
        assert_eq!(list[1].id, c2.id);

        s.rename_conversation(c.id, "바뀐 제목").unwrap();
        assert_eq!(s.conversation(c.id).unwrap().unwrap().title, "바뀐 제목");
        s.delete_conversation(c.id).unwrap();
        assert!(
            s.messages(c.id).unwrap().is_empty(),
            "메시지도 같이 지워진다"
        );
        assert!(s.conversation(c.id).unwrap().is_none());
    }

    #[test]
    fn title_is_first_line_trimmed() {
        assert_eq!(title_from("\n  안녕하세요  \n둘째 줄"), "안녕하세요");
        assert_eq!(title_from(""), "새 대화");
        let long = "가".repeat(50);
        assert_eq!(title_from(&long).chars().count(), 41);
    }
}
