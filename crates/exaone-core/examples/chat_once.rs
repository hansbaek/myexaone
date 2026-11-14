//! 앱의 대화 창과 같은 경로(게이트웨이 스트리밍 + 저장소)로 질문 하나를 보내 본다. 게이트웨이가 켜져 있어야 한다.
//!
//! cargo run -p exaone-core --example chat_once -- "질문"

use std::io::Write;
use std::sync::atomic::AtomicBool;

use exaone_core::chat::{self, ChatEvent, ChatTurn};
use exaone_core::settings::Settings;
use exaone_core::store::{AssistantReply, Store};

fn main() -> anyhow::Result<()> {
    let q = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "안녕? 한 문장으로 답해 줘.".into());
    let settings = Settings::load()?;
    let key = exaone_core::secrets::master_key()?;
    let store = Store::in_memory()?;
    let conv = store.create_conversation("시험")?;
    store.add_user_message(conv.id, &q)?;
    let history: Vec<ChatTurn> = store
        .messages(conv.id)?
        .into_iter()
        .map(|m| ChatTurn {
            role: m.role,
            content: m.content,
        })
        .collect();
    let (mut content, mut reasoning) = (String::new(), String::new());
    let usage = chat::stream(
        &settings.gateway_url(),
        &key,
        &history,
        "low",
        &AtomicBool::new(false),
        &mut |e| match e {
            ChatEvent::Reasoning(t) => reasoning.push_str(&t),
            ChatEvent::Content(t) => {
                print!("{t}");
                let _ = std::io::stdout().flush();
                content.push_str(&t);
            }
        },
    )?;
    let saved = store.add_assistant_message(
        conv.id,
        &AssistantReply {
            content: content.trim().into(),
            reasoning: Some(reasoning.trim().into()),
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            reasoning_tokens: usage.reasoning_tokens,
            error: None,
        },
    )?;
    println!(
        "\n\n[추론 {}자] [저장된 답변 {:?}] [토큰 {:?}/{:?}/{:?}]",
        reasoning.chars().count(),
        saved.content,
        usage.prompt_tokens,
        usage.completion_tokens,
        usage.reasoning_tokens
    );
    Ok(())
}
