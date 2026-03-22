// ============================================================
//  KaoBot — Telegram expense tracker bot
//  ภรรยากรอก "ข้าว 60" → บันทึก Supabase → สรุปยอด
//  ส่งสลิป หรือ /paid 500 → เคลียร์หนี้
// ============================================================
// ParseMode::Markdown is the legacy mode but is intentionally used here because
// our message templates rely on simple *bold* and _italic_ syntax that is
// compatible with it and would require extensive per-character escaping to
// migrate to MarkdownV2 safely.
#![allow(deprecated)]

mod commands;
mod parser;
mod slip;
mod supabase;

use anyhow::Result;
use dotenvy::dotenv;
use teloxide::{
    dispatching::{UpdateFilterExt, UpdateHandler},
    prelude::*,
    types::{Message, Update},
};
use tracing::info;

pub struct Config {
    pub supabase_url: String,
    pub supabase_key: String,
    pub gemini_key: String,
    pub allowed_chat_id: Option<i64>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Config {
            supabase_url: std::env::var("SUPABASE_URL").expect("SUPABASE_URL must be set"),
            supabase_key: std::env::var("SUPABASE_ANON_KEY")
                .expect("SUPABASE_ANON_KEY must be set"),
            gemini_key: std::env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set"),
            allowed_chat_id: std::env::var("ALLOWED_CHAT_ID")
                .ok()
                .and_then(|v| v.parse::<i64>().ok()),
        })
    }
}

pub type BotConfig = std::sync::Arc<Config>;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter("kaobot=debug,teloxide=info")
        .init();

    info!("KaoBot starting...");

    let bot = Bot::from_env();
    let config: BotConfig = std::sync::Arc::new(Config::from_env()?);

    if config.allowed_chat_id.is_none() {
        info!("ALLOWED_CHAT_ID not set — bot will respond to ALL chats");
        info!("   Send a message in your group to see the chat_id in logs");
    }

    Dispatcher::builder(bot, schema())
        .dependencies(dptree::deps![config])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;

    Ok(())
}

fn schema() -> UpdateHandler<anyhow::Error> {
    use dptree::case;

    let command_handler = teloxide::filter_command::<commands::Command, _>()
        .branch(case![commands::Command::Help].endpoint(commands::handle_help))
        .branch(case![commands::Command::Summary].endpoint(commands::handle_summary))
        .branch(case![commands::Command::Today].endpoint(commands::handle_today))
        .branch(case![commands::Command::History].endpoint(commands::handle_history))
        .branch(case![commands::Command::Paid(amount)].endpoint(commands::handle_paid))
        .branch(case![commands::Command::Cancel(id)].endpoint(commands::handle_cancel))
        .branch(case![commands::Command::Clear].endpoint(commands::handle_clear));

    let message_handler = Update::filter_message()
        .branch(command_handler)
        .branch(
            dptree::filter(|msg: Message| msg.photo().is_some()).endpoint(slip::handle_slip_image),
        )
        .branch(dptree::filter(|msg: Message| msg.text().is_some()).endpoint(handle_text_expense));

    dptree::entry().branch(message_handler)
}

async fn handle_text_expense(bot: Bot, msg: Message, config: BotConfig) -> Result<()> {
    let chat_id = msg.chat.id;
    let text = match msg.text() {
        Some(t) => t,
        None => return Ok(()),
    };

    info!("Message from chat_id: {}", chat_id);

    if let Some(allowed) = config.allowed_chat_id {
        if chat_id.0 != allowed {
            info!("Ignoring message from unauthorized chat: {}", chat_id);
            return Ok(());
        }
    }

    if text.starts_with('/') {
        return Ok(());
    }

    match parser::parse_expense(text) {
        Some((item, amount)) => {
            let sender_name = msg
                .from
                .as_ref()
                .and_then(|u| u.username.clone())
                .or_else(|| msg.from.as_ref().map(|u| u.first_name.clone()))
                .unwrap_or_else(|| "unknown".to_string());

            info!(
                "Parsed expense: {} = {} (from {})",
                item, amount, sender_name
            );

            match supabase::insert_expense(
                &config,
                chat_id.0,
                msg.id.0 as i64,
                &item,
                amount,
                &sender_name,
            )
            .await
            {
                Ok(()) => {
                    let total = supabase::get_pending_total(&config, chat_id.0)
                        .await
                        .unwrap_or(0.0);
                    let credit = supabase::get_credit_balance(&config, chat_id.0)
                        .await
                        .unwrap_or(0.0);
                    let net_due = (total - credit).max(0.0);

                    let reply = if credit > 0.01 && net_due < 0.01 {
                        format!(
                            "✅ บันทึกแล้ว: *{}* {:.0} บาท\n\
                             หัก credit แล้ว — ยังไม่ต้องโอน (credit เหลือ *{:.0} บาท*)",
                            item,
                            amount,
                            credit - total,
                        )
                    } else if credit > 0.01 {
                        format!(
                            "✅ บันทึกแล้ว: *{}* {:.0} บาท\n\
                             หัก credit *{:.0} บาท* แล้ว\n\
                             ยอดที่ต้องโอน: *{:.0} บาท*",
                            item, amount, credit, net_due,
                        )
                    } else {
                        format!(
                            "✅ บันทึกแล้ว: *{}* {:.0} บาท\nยอดค้าง: *{:.0} บาท*",
                            item, amount, total
                        )
                    };
                    bot.send_message(chat_id, reply)
                        .parse_mode(teloxide::types::ParseMode::Markdown)
                        .await?;
                }
                Err(e) => {
                    tracing::error!("Supabase insert error: {}", e);
                    bot.send_message(chat_id, "❌ บันทึกไม่สำเร็จ กรุณาลองใหม่")
                        .await?;
                }
            }
        }
        None => {
            info!("Could not parse as expense: {}", text);
        }
    }

    Ok(())
}
