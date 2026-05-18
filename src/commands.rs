// commands.rs — Telegram bot commands
// ParseMode::Markdown is intentionally used (see main.rs for rationale).
#![allow(deprecated)]

use anyhow::Result;
use std::cmp::Reverse;
use teloxide::{prelude::*, utils::command::BotCommands};
use tracing::error;

use crate::{BotConfig, supabase};

#[derive(BotCommands, Clone, Debug)]
#[command(rename_rule = "lowercase", description = "KaoBot commands")]
pub enum Command {
  #[command(description = "แสดงคำสั่งทั้งหมด")]
  Help,
  #[command(description = "ยอดรวมที่ค้างชำระทั้งหมด")]
  Summary,
  #[command(description = "รายการวันนี้")]
  Today,
  #[command(description = "ประวัติรายการ (10 รายการล่าสุด)")]
  History,
  #[command(description = "บันทึกการโอนเงิน เช่น /paid 500")]
  Paid(f64),
  #[command(description = "ยกเลิกรายการ เช่น /cancel 42")]
  Cancel(i64),
  #[command(description = "เคลียร์ทุกรายการ (ใช้หลังโอนแล้ว)")]
  Clear,
}

pub async fn handle_help(bot: Bot, msg: Message) -> Result<()> {
  let text = "*KaoBot — ผู้ช่วยบันทึกค่าใช้จ่าย*\n\n\
        *วิธีบันทึกรายจ่าย:*\n\
        พิมพ์ชื่อรายการ + เว้นวรรค + จำนวนเงิน\n\
        ตัวอย่าง: `ข้าว 60` หรือ `กาแฟ 65.50`\n\n\
        *คำสั่งที่ใช้ได้:*\n\
        /summary — ยอดรวมค้างชำระ\n\
        /today — รายการวันนี้\n\
        /history — 10 รายการล่าสุด\n\
        /paid 500 — บันทึกการโอนเงิน 500 บาท\n\
        /cancel 42 — ยกเลิกรายการ id 42\n\
        /clear — เคลียร์ทุกรายการ\n\n\
        *ส่งสลิป:*\n\
        ส่งรูปสลิปมาใน chat นี้ — bot จะอ่านยอดและเคลียร์ให้อัตโนมัติ";

  bot
    .send_message(msg.chat.id, text)
    .parse_mode(teloxide::types::ParseMode::Markdown)
    .await?;
  Ok(())
}

pub async fn handle_summary(bot: Bot, msg: Message, config: BotConfig) -> Result<()> {
  let chat_id = msg.chat.id;

  let credit = supabase::get_credit_balance(&config, chat_id.0)
    .await
    .unwrap_or(0.0);

  match supabase::get_pending_expenses(&config, chat_id.0).await {
    Ok(expenses) if expenses.is_empty() => {
      let text = if credit > 0.01 {
        format!(
          "ไม่มียอดค้างชำระ\n💰 credit คงเหลือ *{:.2} บาท* — จะหักอัตโนมัติเมื่อมีรายการใหม่",
          credit
        )
      } else {
        "ไม่มียอดค้างชำระ เคลียร์หมดแล้ว".to_string()
      };
      bot
        .send_message(chat_id, text)
        .parse_mode(teloxide::types::ParseMode::Markdown)
        .await?;
    }
    Ok(expenses) => {
      let total: f64 = expenses.iter().map(|e| e.amount).sum();
      let mut lines = vec!["*รายการค้างชำระ:*\n".to_string()];

      for (i, e) in expenses.iter().enumerate() {
        let date = e
          .created_at
          .with_timezone(&chrono::FixedOffset::east_opt(7 * 3600).unwrap())
          .format("%d/%m %H:%M");
        lines.push(format!(
          "`{:>3}.` {} — *{:.2}฿* _{}_",
          e.id, e.item, e.amount, date
        ));
        if i == 19 && expenses.len() > 20 {
          lines.push(format!("_...และอีก {} รายการ_", expenses.len() - 20));
          break;
        }
      }

      let net_due = (total - credit).max(0.0);
      lines.push(format!("\n*รวมทั้งหมด: {:.2} บาท*", total));
      if credit > 0.01 {
        lines.push(format!("credit คงเหลือ: *{:.2} บาท*", credit));
        lines.push(format!("ยอดที่ต้องโอน: *{:.2} บาท*", net_due));
      }
      lines.push("\nโอนแล้วส่งสลิป หรือพิมพ์ `/paid <จำนวน>`".to_string());

      bot
        .send_message(chat_id, lines.join("\n"))
        .parse_mode(teloxide::types::ParseMode::Markdown)
        .await?;
    }
    Err(e) => {
      error!("Summary error: {}", e);
      bot
        .send_message(chat_id, "❌ ดึงข้อมูลไม่ได้ กรุณาลองใหม่")
        .await?;
    }
  }

  Ok(())
}

pub async fn handle_today(bot: Bot, msg: Message, config: BotConfig) -> Result<()> {
  let chat_id = msg.chat.id;

  match supabase::get_today_expenses(&config, chat_id.0).await {
    Ok(expenses) if expenses.is_empty() => {
      bot.send_message(chat_id, "วันนี้ยังไม่มีรายการ").await?;
    }
    Ok(expenses) => {
      let total: f64 = expenses.iter().map(|e| e.amount).sum();
      let mut lines = vec!["*รายการวันนี้:*\n".to_string()];

      for e in &expenses {
        let time = e
          .created_at
          .with_timezone(&chrono::FixedOffset::east_opt(7 * 3600).unwrap())
          .format("%H:%M");
        lines.push(format!("• {} — *{:.2}฿* _{}_", e.item, e.amount, time));
      }

      lines.push(format!("\nรวมวันนี้: *{:.2} บาท*", total));

      bot
        .send_message(chat_id, lines.join("\n"))
        .parse_mode(teloxide::types::ParseMode::Markdown)
        .await?;
    }
    Err(e) => {
      tracing::error!("Today error: {}", e);
      bot.send_message(chat_id, "❌ ดึงข้อมูลไม่ได้").await?;
    }
  }

  Ok(())
}

pub async fn handle_history(bot: Bot, msg: Message, config: BotConfig) -> Result<()> {
  let chat_id = msg.chat.id;

  match supabase::get_pending_expenses(&config, chat_id.0).await {
    Ok(mut expenses) => {
      expenses.sort_by_key(|b| Reverse(b.created_at));
      let recent: Vec<_> = expenses.into_iter().take(10).collect();

      if recent.is_empty() {
        bot.send_message(chat_id, "ไม่มีรายการค้างชำระ").await?;
        return Ok(());
      }

      let mut lines = vec!["*10 รายการล่าสุด:*\n".to_string()];
      for e in &recent {
        let date = e
          .created_at
          .with_timezone(&chrono::FixedOffset::east_opt(7 * 3600).unwrap())
          .format("%d/%m %H:%M");
        lines.push(format!(
          "`#{}`  {} — *{:.2}฿*  _{}_ (by {})",
          e.id, e.item, e.amount, date, e.paid_by
        ));
      }

      bot
        .send_message(chat_id, lines.join("\n"))
        .parse_mode(teloxide::types::ParseMode::Markdown)
        .await?;
    }
    Err(e) => {
      tracing::error!("History error: {}", e);
      bot.send_message(chat_id, "❌ ดึงข้อมูลไม่ได้").await?;
    }
  }

  Ok(())
}

pub async fn handle_paid(bot: Bot, msg: Message, config: BotConfig, cmd: Command) -> Result<()> {
  let Command::Paid(amount) = cmd else {
    return Ok(());
  };
  let chat_id = msg.chat.id;

  if amount <= 0.0 {
    bot.send_message(chat_id, "❌ จำนวนเงินต้องมากกว่า 0").await?;
    return Ok(());
  }

  let reply = settle_payment(
    &config,
    chat_id.0,
    Some(msg.id.0 as i64),
    amount,
    "manual",
    None,
  )
  .await?;

  bot
    .send_message(chat_id, reply)
    .parse_mode(teloxide::types::ParseMode::Markdown)
    .await?;

  Ok(())
}

/// Shared logic for receiving payments (both /paid and slip)
///
/// Steps:
/// 1. Fetch pending total and current credit
/// 2. Record payment record
/// 3. Clear all pending items
/// 4. Calculate difference — if overpaid, store as credit for next items
/// 5. Return reply message
pub async fn settle_payment(
  config: &BotConfig,
  chat_id: i64,
  message_id: Option<i64>,
  paid_amount: f64,
  method: &str,
  note: Option<&str>,
) -> Result<String> {
  let pending_total = supabase::get_pending_total(config, chat_id)
    .await
    .unwrap_or(0.0);

  let current_credit = supabase::get_credit_balance(config, chat_id)
    .await
    .unwrap_or(0.0);

  supabase::insert_payment(config, chat_id, message_id, paid_amount, method, note).await?;

  let cleared = supabase::clear_all_expenses(config, chat_id).await?;

  let net = current_credit + paid_amount - pending_total;

  let new_credit = if net > 0.01 { net } else { 0.0 };
  if let Err(e) = supabase::upsert_credit(config, chat_id, new_credit).await {
    error!("Failed to upsert credit: {}", e);
  }

  let mut lines = vec![
    format!("✅ *บันทึกการโอน {:.2} บาท*", paid_amount),
    format!("เคลียร์ {} รายการ", cleared),
    format!("ยอดค้างก่อนหน้า: *{:.2} บาท*", pending_total),
  ];

  if current_credit > 0.01 {
    lines.push(format!("credit เดิม: *{:.2} บาท*", current_credit));
  }

  if net > 0.01 {
    lines.push(format!(
      "โอนเกิน *{:.2} บาท* — เก็บเป็น credit หักรายการหน้าอัตโนมัติ",
      net
    ));
  } else if net < -0.01 {
    lines.push(format!("⚠️ ยังขาดอยู่ *{:.2} บาท*", net.abs()));
  } else {
    lines.push("ยอดตรงพอดี".to_string());
  }

  Ok(lines.join("\n"))
}

pub async fn handle_cancel(bot: Bot, msg: Message, config: BotConfig, cmd: Command) -> Result<()> {
  let Command::Cancel(id) = cmd else {
    return Ok(());
  };
  let chat_id = msg.chat.id;

  match supabase::cancel_expense(&config, chat_id.0, id).await {
    Ok(true) => {
      let total = supabase::get_pending_total(&config, chat_id.0)
        .await
        .unwrap_or(0.0);
      bot
        .send_message(
          chat_id,
          format!("ยกเลิกรายการ #{} แล้ว\nยอดค้างเหลือ: *{:.2} บาท*", id, total),
        )
        .parse_mode(teloxide::types::ParseMode::Markdown)
        .await?;
    }
    Ok(false) => {
      bot
        .send_message(chat_id, format!("❌ ไม่พบรายการ #{} หรือถูกเคลียร์ไปแล้ว", id))
        .await?;
    }
    Err(e) => {
      tracing::error!("Cancel error: {}", e);
      bot.send_message(chat_id, "❌ ยกเลิกไม่สำเร็จ").await?;
    }
  }

  Ok(())
}

pub async fn handle_clear(bot: Bot, msg: Message, config: BotConfig) -> Result<()> {
  let chat_id = msg.chat.id;

  let total = supabase::get_pending_total(&config, chat_id.0)
    .await
    .unwrap_or(0.0);

  let cleared = supabase::clear_all_expenses(&config, chat_id.0).await?;

  if cleared == 0 {
    bot.send_message(chat_id, "ไม่มีรายการค้างชำระอยู่แล้ว").await?;
  } else {
    bot
      .send_message(
        chat_id,
        format!("✅ *เคลียร์แล้ว!*\n{} รายการ รวม *{:.2} บาท*", cleared, total),
      )
      .parse_mode(teloxide::types::ParseMode::Markdown)
      .await?;
  }

  Ok(())
}
