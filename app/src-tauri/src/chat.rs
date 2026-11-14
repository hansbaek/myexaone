//! 대화 창. 저장소와 스트리밍 호출은 exaone-core 에 있다.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use exaone_core::chat::{self, ChatEvent, ChatTurn};
use exaone_core::settings::Settings;
use exaone_core::store::{self, AssistantReply, Conversation, Message, Store};
use exaone_core::{gateway, secrets};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

const NEW_TITLE: &str = "새 대화";

#[derive(Default)]
pub struct ChatState {
    /// 대화 id → 멈춤 신호. 답변을 받는 중인 대화만 들어 있다.
    running: Mutex<HashMap<i64, Arc<AtomicBool>>>,
}

type Shared = Arc<ChatState>;

fn err(e: impl std::fmt::Display) -> String {
    format!("{e:#}")
}

fn db() -> Result<Store, String> {
    Store::open_default().map_err(err)
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeltaEvent {
    conversation_id: i64,
    reasoning: Option<String>,
    content: Option<String>,
    /// 서버 혼잡으로 다시 시도 중이면 (몇 번째, 최대)
    retry: Option<(u32, u32)>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DoneEvent {
    conversation_id: i64,
    message: Message,
    title: Option<String>,
}

#[tauri::command(async)]
pub fn chat_list() -> Result<Vec<Conversation>, String> {
    db()?.conversations().map_err(err)
}

#[tauri::command(async)]
pub fn chat_messages(id: i64) -> Result<Vec<Message>, String> {
    db()?.messages(id).map_err(err)
}

#[tauri::command(async)]
pub fn chat_create() -> Result<Conversation, String> {
    db()?.create_conversation(NEW_TITLE).map_err(err)
}

#[tauri::command(async)]
pub fn chat_rename(id: i64, title: String) -> Result<(), String> {
    let title = title.trim();
    if title.is_empty() {
        return Ok(());
    }
    db()?.rename_conversation(id, title).map_err(err)
}

#[tauri::command(async)]
pub fn chat_delete(state: State<'_, Shared>, id: i64) -> Result<(), String> {
    chat_stop(state, id);
    db()?.delete_conversation(id).map_err(err)
}

#[tauri::command(async)]
pub fn chat_stop(state: State<'_, Shared>, id: i64) {
    if let Some(flag) = state
        .running
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&id)
    {
        flag.store(true, Ordering::Relaxed);
    }
}

#[tauri::command(async)]
pub fn chat_running(state: State<'_, Shared>) -> Vec<i64> {
    state
        .running
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .keys()
        .copied()
        .collect()
}

/// 질문을 저장하고 답변 받기를 시작한다. 답변은 chat-delta / chat-done 이벤트로 온다.
#[tauri::command(async)]
pub fn chat_send(
    app: AppHandle,
    state: State<'_, Shared>,
    id: i64,
    text: String,
    effort: String,
) -> Result<Message, String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("내용이 비어 있습니다".into());
    }
    if !["low", "high", "max"].contains(&effort.as_str()) {
        return Err(format!("알 수 없는 추론 강도: {effort}"));
    }
    let settings = Settings::load().map_err(err)?;
    if !gateway::is_healthy(settings.gateway_port) {
        return Err("게이트웨이가 꺼져 있습니다. 대시보드에서 먼저 켜세요.".into());
    }
    let key = secrets::master_key().map_err(err)?;

    let flag = Arc::new(AtomicBool::new(false));
    {
        let mut running = state.running.lock().unwrap_or_else(|e| e.into_inner());
        if running.contains_key(&id) {
            return Err("이 대화는 아직 답변을 받는 중입니다".into());
        }
        running.insert(id, flag.clone());
    }

    let store = db()?;
    let user = match store.add_user_message(id, &text) {
        Ok(m) => m,
        Err(e) => {
            state
                .running
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&id);
            return Err(err(e));
        }
    };
    // 실패한 답변(내용 없음)은 대화 맥락에서 뺀다.
    let history: Vec<ChatTurn> = store
        .messages(id)
        .map_err(err)?
        .into_iter()
        .filter(|m| !m.content.trim().is_empty())
        .map(|m| ChatTurn {
            role: m.role,
            content: m.content,
        })
        .collect();
    let title = store
        .conversation(id)
        .ok()
        .flatten()
        .filter(|c| c.title == NEW_TITLE)
        .map(|_| store::title_from(&text));
    if let Some(t) = &title {
        let _ = store.rename_conversation(id, t);
    }

    let state = state.inner().clone();
    std::thread::spawn(move || {
        let mut content = String::new();
        let mut reasoning = String::new();
        let started = std::time::Instant::now();
        let mut first_token: Option<i64> = None;
        let mut retries = 0i64;
        let result = chat::stream(
            &settings.gateway_url(),
            &key,
            &history,
            &effort,
            &flag,
            &mut |ev| {
                let (r, c, retry) = match ev {
                    ChatEvent::Reasoning(t) => {
                        reasoning.push_str(&t);
                        (Some(t), None, None)
                    }
                    ChatEvent::Content(t) => {
                        content.push_str(&t);
                        (None, Some(t), None)
                    }
                    ChatEvent::Retrying { attempt, max } => {
                        retries = i64::from(attempt);
                        (None, None, Some((attempt, max)))
                    }
                };
                if retry.is_none() && first_token.is_none() {
                    first_token = Some(started.elapsed().as_millis() as i64);
                }
                let _ = app.emit(
                    "chat-delta",
                    DeltaEvent {
                        conversation_id: id,
                        reasoning: r,
                        content: c,
                        retry,
                    },
                );
            },
        );
        let total_ms = started.elapsed().as_millis() as i64;
        let stopped = flag.load(Ordering::Relaxed);
        let mut reply = AssistantReply {
            content: content.trim().to_string(),
            reasoning: Some(reasoning.trim().to_string()).filter(|s| !s.is_empty()),
            first_token_ms: first_token,
            total_ms: Some(total_ms),
            retries: Some(retries).filter(|n| *n > 0),
            ..Default::default()
        };
        match result {
            Ok(u) => {
                reply.prompt_tokens = u.prompt_tokens;
                reply.completion_tokens = u.completion_tokens;
                reply.reasoning_tokens = u.reasoning_tokens;
                if stopped {
                    reply.error = Some("중간에 멈췄습니다".into());
                }
            }
            Err(e) => reply.error = Some(err(e)),
        }
        let saved = Store::open_default().and_then(|s| s.add_assistant_message(id, &reply));
        state
            .running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
        if let Ok(message) = saved {
            let _ = app.emit(
                "chat-done",
                DoneEvent {
                    conversation_id: id,
                    message,
                    title,
                },
            );
        }
    });
    Ok(user)
}
