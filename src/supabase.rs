// supabase.rs — Supabase REST API client
//
// Credit system:
// When a transfer exceeds the pending amount, the excess is stored as "credit"
// New expenses are deducted from credit first; if credit is sufficient, no transfer is needed

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::BotConfig;

// ---- Data models ----

#[derive(Debug, Serialize)]
pub struct NewExpense<'a> {
  pub item: &'a str,
  pub amount: f64,
  pub paid_by: &'a str,
  pub chat_id: i64,
  pub message_id: i64,
}

#[derive(Debug, Deserialize)]
pub struct Expense {
  pub id: i64,
  pub item: String,
  pub amount: f64,
  pub paid_by: String,
  // Required for Supabase JSON deserialization; not read directly in application logic.
  #[allow(dead_code)]
  pub is_cleared: bool,
  pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct PendingSummary {
  // Required for Supabase JSON deserialization; only total_amount is used directly.
  #[allow(dead_code)]
  pub item_count: Option<i64>,
  pub total_amount: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct NewPayment<'a> {
  pub amount: f64,
  pub method: &'a str,
  pub chat_id: i64,
  pub message_id: Option<i64>,
  pub note: Option<&'a str>,
}

#[derive(Debug, Deserialize)]
pub struct CreditBalance {
  #[allow(dead_code)]
  pub chat_id: i64,
  pub balance: f64,
}

// ---- Helper ----

fn build_headers(config: &BotConfig) -> reqwest::header::HeaderMap {
  let mut map = reqwest::header::HeaderMap::new();
  map.insert(
    "apikey",
    config
      .supabase_key
      .parse()
      .expect("supabase_key is a valid header value"),
  );
  map.insert(
    "Authorization",
    format!("Bearer {}", config.supabase_key)
      .parse()
      .expect("Authorization header is a valid header value"),
  );
  map.insert(
    "Content-Type",
    "application/json"
      .parse()
      .expect("Content-Type is a valid header value"),
  );
  map.insert(
    "Prefer",
    "return=minimal"
      .parse()
      .expect("Prefer is a valid header value"),
  );
  map
}

fn build_headers_with_representation(config: &BotConfig) -> reqwest::header::HeaderMap {
  let mut map = build_headers(config);
  map.insert(
    "Prefer",
    "return=representation"
      .parse()
      .expect("Prefer is a valid header value"),
  );
  map
}

// ---- Operations ----

/// Record a new expense
pub async fn insert_expense(
  config: &BotConfig,
  chat_id: i64,
  message_id: i64,
  item: &str,
  amount: f64,
  paid_by: &str,
) -> Result<()> {
  let body = NewExpense {
    item,
    amount,
    paid_by,
    chat_id,
    message_id,
  };

  let res = Client::new()
    .post(format!("{}/rest/v1/expenses", config.supabase_url))
    .headers(build_headers(config))
    .json(&body)
    .send()
    .await
    .context("Failed to send request to Supabase")?;

  if !res.status().is_success() {
    let status = res.status();
    let text = res.text().await.unwrap_or_default();
    anyhow::bail!("Supabase error {}: {}", status, text);
  }

  Ok(())
}

/// Get total pending amount
pub async fn get_pending_total(config: &BotConfig, chat_id: i64) -> Result<f64> {
  let res = Client::new()
    .get(format!(
      "{}/rest/v1/pending_summary?chat_id=eq.{}",
      config.supabase_url, chat_id
    ))
    .headers(build_headers(config))
    .send()
    .await
    .context("Failed to query pending_summary")?;

  let summaries: Vec<PendingSummary> = res
    .json()
    .await
    .context("Failed to parse pending_summary response")?;

  Ok(
    summaries
      .first()
      .and_then(|s| s.total_amount)
      .unwrap_or(0.0),
  )
}

/// Get all pending expenses
pub async fn get_pending_expenses(config: &BotConfig, chat_id: i64) -> Result<Vec<Expense>> {
  let res = Client::new()
    .get(format!(
      "{}/rest/v1/expenses?chat_id=eq.{}&is_cleared=eq.false&order=created_at.asc",
      config.supabase_url, chat_id
    ))
    .headers(build_headers_with_representation(config))
    .send()
    .await
    .context("Failed to query expenses")?;

  let expenses: Vec<Expense> = res.json().await.context("Failed to parse expenses")?;

  Ok(expenses)
}

/// Get today's expenses (Asia/Bangkok)
pub async fn get_today_expenses(config: &BotConfig, chat_id: i64) -> Result<Vec<Expense>> {
  // Calculate start of today in Thailand time (UTC+7)
  // 00:00 +07:00 = 17:00 UTC previous day
  let bkk = chrono::FixedOffset::east_opt(7 * 3600).unwrap();
  let now_bkk = Utc::now().with_timezone(&bkk);
  let today_midnight_bkk = now_bkk.date_naive().and_hms_opt(0, 0, 0).unwrap();
  // Convert back to UTC: subtract 7 hour offset
  let today_start_utc = today_midnight_bkk - chrono::TimeDelta::hours(7);
  let today_start = today_start_utc.and_utc().to_rfc3339();

  let res = Client::new()
        .get(format!(
            "{}/rest/v1/expenses?chat_id=eq.{}&is_cleared=eq.false&created_at=gte.{}&order=created_at.asc",
            config.supabase_url, chat_id, today_start
        ))
        .headers(build_headers_with_representation(config))
        .send()
        .await
        .context("Failed to query today's expenses")?;

  let expenses: Vec<Expense> = res
    .json()
    .await
    .context("Failed to parse today's expenses")?;

  Ok(expenses)
}

/// Clear all expenses (after transfer)
pub async fn clear_all_expenses(config: &BotConfig, chat_id: i64) -> Result<u64> {
  let expenses = get_pending_expenses(config, chat_id).await?;
  let count = expenses.len() as u64;

  if count == 0 {
    return Ok(0);
  }

  let res = Client::new()
    .patch(format!(
      "{}/rest/v1/expenses?chat_id=eq.{}&is_cleared=eq.false",
      config.supabase_url, chat_id
    ))
    .headers(build_headers(config))
    .json(&serde_json::json!({
        "is_cleared": true,
        "cleared_at": Utc::now().to_rfc3339()
    }))
    .send()
    .await
    .context("Failed to clear expenses")?;

  if !res.status().is_success() {
    let status = res.status();
    let text = res.text().await.unwrap_or_default();
    anyhow::bail!("Supabase clear error {}: {}", status, text);
  }

  Ok(count)
}

/// Delete a single expense by id (in case of incorrect entry)
/// Uses Prefer: return=representation to verify deletion actually occurred
pub async fn cancel_expense(config: &BotConfig, chat_id: i64, id: i64) -> Result<bool> {
  let res = Client::new()
    .delete(format!(
      "{}/rest/v1/expenses?id=eq.{}&chat_id=eq.{}&is_cleared=eq.false",
      config.supabase_url, id, chat_id
    ))
    .headers(build_headers_with_representation(config))
    .send()
    .await
    .context("Failed to delete expense")?;

  if !res.status().is_success() {
    return Ok(false);
  }

  // Supabase returns the deleted rows when using return=representation
  // If the array is empty, no row was actually deleted
  let deleted: Vec<serde_json::Value> = res.json().await.unwrap_or_default();
  Ok(!deleted.is_empty())
}

/// Get credit balance for a chat (overpaid amount)
/// Returns 0.0 if no credit exists yet
pub async fn get_credit_balance(config: &BotConfig, chat_id: i64) -> Result<f64> {
  let res = Client::new()
    .get(format!(
      "{}/rest/v1/credit_balance?chat_id=eq.{}&select=balance",
      config.supabase_url, chat_id
    ))
    .headers(build_headers_with_representation(config))
    .send()
    .await
    .context("Failed to query credit_balance")?;

  let rows: Vec<CreditBalance> = res
    .json()
    .await
    .context("Failed to parse credit_balance response")?;

  Ok(rows.first().map(|r| r.balance).unwrap_or(0.0))
}

/// Update credit balance (upsert)
/// Inserts new row if none exists, otherwise updates
pub async fn upsert_credit(config: &BotConfig, chat_id: i64, new_balance: f64) -> Result<()> {
  let mut headers = build_headers(config);
  headers.insert(
    "Prefer",
    "resolution=merge-duplicates,return=minimal"
      .parse()
      .expect("Prefer header is valid"),
  );

  let res = Client::new()
    .post(format!("{}/rest/v1/credit_balance", config.supabase_url))
    .headers(headers)
    .json(&serde_json::json!({
        "chat_id": chat_id,
        "balance": new_balance,
        "updated_at": Utc::now().to_rfc3339()
    }))
    .send()
    .await
    .context("Failed to upsert credit_balance")?;

  if !res.status().is_success() {
    let status = res.status();
    let text = res.text().await.unwrap_or_default();
    anyhow::bail!("Supabase credit upsert error {}: {}", status, text);
  }

  Ok(())
}

/// Record a payment
pub async fn insert_payment(
  config: &BotConfig,
  chat_id: i64,
  message_id: Option<i64>,
  amount: f64,
  method: &str,
  note: Option<&str>,
) -> Result<()> {
  let body = NewPayment {
    amount,
    method,
    chat_id,
    message_id,
    note,
  };

  let res = Client::new()
    .post(format!("{}/rest/v1/payments", config.supabase_url))
    .headers(build_headers(config))
    .json(&body)
    .send()
    .await
    .context("Failed to insert payment")?;

  if !res.status().is_success() {
    let status = res.status();
    let text = res.text().await.unwrap_or_default();
    anyhow::bail!("Supabase payment error {}: {}", status, text);
  }

  Ok(())
}
