// slip.rs — Read transfer slip amounts using Gemini Vision API
// ParseMode::Markdown is intentionally used (see main.rs for rationale).
#![allow(deprecated)]

use anyhow::Result;
use base64::{Engine as _, engine::general_purpose};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use teloxide::prelude::*;
use teloxide::types::FileId;
use tracing::{error, info, warn};

use crate::{BotConfig, supabase};

// ---- Gemini API request types ----

#[derive(Serialize)]
struct GeminiRequest {
  contents: Vec<GeminiContent>,
  #[serde(rename = "generationConfig")]
  generation_config: GenerationConfig,
}

#[derive(Serialize)]
struct GeminiContent {
  parts: Vec<GeminiPart>,
}

/// Each element in `parts` is either plain text or an inline image.
/// `#[serde(untagged)]` tells serde to serialise whichever variant is active
/// without injecting a discriminator field — exactly what the Gemini REST API
/// expects.
#[derive(Serialize)]
#[serde(untagged)]
enum GeminiPart {
  Text {
    text: String,
  },
  InlineData {
    #[serde(rename = "inlineData")]
    inline_data: InlineData,
  },
}

#[derive(Serialize)]
struct InlineData {
  #[serde(rename = "mimeType")]
  mime_type: String,
  data: String,
}

#[derive(Serialize)]
struct ThinkingConfig {
  #[serde(rename = "thinkingBudget")]
  thinking_budget: u32,
}

#[derive(Serialize)]
struct GenerationConfig {
  temperature: f32,
  #[serde(rename = "maxOutputTokens")]
  max_output_tokens: u32,
  #[serde(rename = "responseMimeType")]
  response_mime_type: String,
  #[serde(rename = "thinkingConfig")]
  thinking_config: ThinkingConfig,
}

// ---- Gemini API response types ----
//
// Gemini returns:
// {
//   "candidates": [{
//     "content": {
//       "parts": [{ "text": "..." }]
//     }
//   }]
// }

#[derive(Deserialize)]
struct GeminiResponse {
  candidates: Vec<GeminiCandidate>,
}

#[derive(Deserialize)]
struct GeminiCandidate {
  content: GeminiResponseContent,
  #[serde(rename = "finishReason")]
  finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct GeminiResponseContent {
  parts: Vec<GeminiResponsePart>,
}

#[derive(Deserialize)]
struct GeminiResponsePart {
  text: Option<String>,
}

/// Extract JSON object from Gemini response robustly
///
/// Supports all formats Gemini may return:
/// 1. Raw JSON:           {"amount": 100.0, "note": "..."}
/// 2. ```json fence:      ```json\n{...}\n```
/// 3. ``` fence:          ```\n{...}\n```
/// 4. JSON in text:       "the amount is {"amount": 100.0} thanks"
/// 5. Truncated JSON:     {"amount": 100.  ← detect and return None
fn extract_json_object(raw: &str) -> Option<String> {
  // Find first { and last } to extract JSON object
  let start = raw.find('{')?;
  let end = raw.rfind('}')?;
  if end < start {
    return None;
  }
  Some(raw[start..=end].to_string())
}

// ---- Public output type ----

#[derive(Debug)]
pub struct SlipInfo {
  pub amount: Option<f64>,
  pub raw_text: String,
}

// ---- Private helpers ----

/// Download file from Telegram and encode as base64
async fn download_photo_base64(bot: &Bot, file_id: FileId) -> Result<String> {
  let file = bot.get_file(file_id.clone()).await?;
  let url = format!(
    "https://api.telegram.org/file/bot{}/{}",
    bot.token(),
    file.path
  );
  let bytes = reqwest::get(url).await?.bytes().await?;
  Ok(general_purpose::STANDARD.encode(bytes))
}

// ---- Public API ----

/// Send image to Gemini to read amount from slip
///
/// ใช้ `gemini-2.0-flash` — รองรับ multimodal (image + text) ใน single request
/// โดยส่ง base64-encoded JPEG ผ่าน `inlineData` part ตาม Gemini REST API spec
pub async fn read_slip_with_gemini(gemini_key: &str, image_base64: &str) -> Result<SlipInfo> {
  // Shortest prompt possible — shorter means less chance for Gemini to add extra text
  // Using English to reduce token overhead
  let prompt = "Read the transfer amount from this Thai bank slip.\n\
        Reply with ONLY a raw JSON object, no markdown, no explanation:\n\
        {\"amount\": 100.00, \"note\": \"100 THB\"}\n\
        If unreadable: {\"amount\": null, \"note\": \"unreadable\"}";

  let request = GeminiRequest {
    contents: vec![GeminiContent {
      parts: vec![
        // Slip image first, then prompt text
        GeminiPart::InlineData {
          inline_data: InlineData {
            mime_type: "image/jpeg".to_string(),
            data: image_base64.to_string(),
          },
        },
        GeminiPart::Text {
          text: prompt.to_string(),
        },
      ],
    }],
    generation_config: GenerationConfig {
      // temperature 0 → deterministic output, suitable for structured extraction
      // 1024 tokens buffer for overhead in case model inserts thinking tokens
      max_output_tokens: 1024,
      // Enforce JSON mode at API level — Gemini will not include markdown fence or
      // explanatory text, only a valid JSON object
      response_mime_type: "application/json".to_string(),
      temperature: 0.0,
      // Disable thinking mode — this model is a thinking model (gemini-3-flash-preview)
      // thinking tokens consume budget in maxOutputTokens, causing JSON output to be truncated
      // thinkingBudget: 0 = thinking fully disabled
      thinking_config: ThinkingConfig { thinking_budget: 0 },
    },
  };

  // Gemini REST API: API key ส่งผ่าน query parameter `key`
  let url = format!(
    "https://generativelanguage.googleapis.com/v1beta/models/gemini-3-flash-preview:generateContent?key={}",
    gemini_key
  );

  let response = Client::new()
    .post(&url)
    .header("Content-Type", "application/json")
    .json(&request)
    .send()
    .await?;

  if !response.status().is_success() {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    anyhow::bail!("Gemini API error {}: {}", status, body);
  }

  // Read raw body before deserialize so logs always show full response
  let raw_body = response.text().await?;
  info!("Gemini raw response body: {}", raw_body);

  let gemini_resp: GeminiResponse = serde_json::from_str(&raw_body).map_err(|e| {
    anyhow::anyhow!(
      "Failed to parse Gemini response: {} — body: {}",
      e,
      raw_body
    )
  })?;

  // Get first candidate with finishReason
  let candidate = gemini_resp.candidates.into_iter().next();

  // Check finishReason — Gemini may send as string "MAX_TOKENS" or "STOP"
  // Some versions send as numbers "2" (MAX_TOKENS) or "1" (STOP)
  if let Some(ref c) = candidate {
    let reason = c.finish_reason.as_deref().unwrap_or("");
    let is_truncated = reason == "MAX_TOKENS" || reason == "2";
    if is_truncated {
      warn!(
        "Gemini response truncated (finishReason={}) — raw: {}",
        reason, raw_body
      );
      return Ok(SlipInfo {
        amount: None,
        raw_text: "อ่านยอดไม่ได้ (response ถูกตัด — ลองใหม่อีกครั้ง)".to_string(),
      });
    }
    info!("Gemini finishReason: {}", reason);
  }

  let text = candidate
    .and_then(|c| c.content.parts.into_iter().find_map(|p| p.text))
    .unwrap_or_default();

  info!("Gemini extracted text: {:?}", text);

  // Extract JSON object from response regardless of Gemini's format
  let json_str = extract_json_object(&text);

  let parsed: serde_json::Value = match json_str.as_deref() {
    Some(s) => serde_json::from_str(s).unwrap_or_else(|e| {
      warn!("JSON parse failed after extraction: {} — raw: {}", e, s);
      serde_json::json!({"amount": null, "note": text.trim()})
    }),
    None => {
      warn!("No JSON object found in Gemini response: {}", text);
      serde_json::json!({"amount": null, "note": text.trim()})
    }
  };

  let amount = parsed["amount"].as_f64();
  let note = parsed["note"].as_str().unwrap_or(text.trim()).to_string();

  Ok(SlipInfo {
    amount,
    raw_text: note,
  })
}

/// Handler: receive image → read slip with Gemini → record payment → clear items
pub async fn handle_slip_image(bot: Bot, msg: Message, config: BotConfig) -> Result<()> {
  let chat_id = msg.chat.id;

  // Check allowed chat
  if let Some(allowed) = config.allowed_chat_id {
    if chat_id.0 != allowed {
      return Ok(());
    }
  }

  let photos = match msg.photo() {
    Some(p) => p,
    None => return Ok(()),
  };

  // Select highest resolution image (last item in array)
  let best_photo = match photos.last() {
    Some(p) => p,
    None => return Ok(()),
  };

  // Notify user that processing is in progress
  let processing_msg = bot.send_message(chat_id, "กำลังอ่านสลิป...").await?;

  // Download image from Telegram
  let image_b64 = match download_photo_base64(&bot, best_photo.file.id.clone()).await {
    Ok(b64) => b64,
    Err(e) => {
      error!("Download photo error: {}", e);
      bot
        .edit_message_text(chat_id, processing_msg.id, "❌ ดาวน์โหลดรูปไม่ได้ กรุณาลองใหม่")
        .await?;
      return Ok(());
    }
  };

  // Send image to Gemini for reading
  match read_slip_with_gemini(&config.gemini_key, &image_b64).await {
    Ok(slip_info) => {
      if let Some(amount) = slip_info.amount {
        // Use settle_payment to record payment, clear items, and manage credit
        let note = slip_info.raw_text.clone();
        match crate::commands::settle_payment(
          &config,
          chat_id.0,
          Some(msg.id.0 as i64),
          amount,
          "slip",
          Some(&note),
        )
        .await
        {
          Ok(settle_reply) => {
            let reply = format!(
              "✅ *อ่านสลิปสำเร็จ*\n\
                             ยอดโอน: *{:.2} บาท*\n\
                             {}\n\
                             {}",
              amount, slip_info.raw_text, settle_reply
            );
            bot
              .edit_message_text(chat_id, processing_msg.id, reply)
              .parse_mode(teloxide::types::ParseMode::Markdown)
              .await?;
          }
          Err(e) => {
            error!("settle_payment error: {}", e);
            bot
              .edit_message_text(
                chat_id,
                processing_msg.id,
                "❌ บันทึกการชำระเงินไม่สำเร็จ กรุณาลองใหม่",
              )
              .await?;
          }
        }
      } else {
        // Gemini couldn't read amount — ask user to confirm manually
        let credit = supabase::get_credit_balance(&config, chat_id.0)
          .await
          .unwrap_or(0.0);
        let raw_pending = supabase::get_pending_total(&config, chat_id.0)
          .await
          .unwrap_or(0.0);
        let net_due = (raw_pending - credit).max(0.0);

        let reply = format!(
          "ได้รับสลิปแล้ว แต่อ่านยอดไม่ได้\n\
                     _{}_\n\n\
                     ยอดค้าง: *{:.2} บาท*{}\n\
                     กรุณาพิมพ์ `/paid <จำนวน>` เพื่อบันทึกเอง",
          slip_info.raw_text,
          net_due,
          if credit > 0.01 {
            format!(" _(หลังหัก credit {:.2}฿)_", credit)
          } else {
            String::new()
          }
        );

        bot
          .edit_message_text(chat_id, processing_msg.id, reply)
          .parse_mode(teloxide::types::ParseMode::Markdown)
          .await?;
      }
    }
    Err(e) => {
      error!("Gemini API error: {}", e);
      let pending_total = supabase::get_pending_total(&config, chat_id.0)
        .await
        .unwrap_or(0.0);
      let credit = supabase::get_credit_balance(&config, chat_id.0)
        .await
        .unwrap_or(0.0);
      let net_due = (pending_total - credit).max(0.0);

      bot
        .edit_message_text(
          chat_id,
          processing_msg.id,
          format!(
            "❌ อ่านสลิปไม่สำเร็จ\n\
                     ยอดค้าง: *{:.2} บาท*{}\n\
                     กรุณาพิมพ์ `/paid <จำนวน>` แทน",
            net_due,
            if credit > 0.01 {
              format!(" _(หลังหัก credit {:.2}฿)_", credit)
            } else {
              String::new()
            }
          ),
        )
        .parse_mode(teloxide::types::ParseMode::Markdown)
        .await?;
    }
  }

  Ok(())
}

// ---- Unit tests ----

#[cfg(test)]
mod tests {
  use super::*;

  /// Test parsing valid JSON response from Gemini
  #[test]
  fn test_parse_valid_gemini_json() {
    let raw = r#"{"amount": 500.00, "note": "โอนเงิน 500 บาท เมื่อ 18/03 10:30"}"#;
    let parsed: serde_json::Value = serde_json::from_str(raw.trim()).unwrap();
    assert_eq!(parsed["amount"].as_f64(), Some(500.0));
    assert_eq!(
      parsed["note"].as_str(),
      Some("โอนเงิน 500 บาท เมื่อ 18/03 10:30")
    );
  }

  /// Test fallback when amount cannot be read (amount = null)
  #[test]
  fn test_parse_null_amount() {
    let raw = r#"{"amount": null, "note": "อ่านยอดไม่ได้"}"#;
    let parsed: serde_json::Value = serde_json::from_str(raw.trim()).unwrap();
    assert!(parsed["amount"].is_null());
    assert_eq!(parsed["note"].as_str(), Some("อ่านยอดไม่ได้"));
  }

  /// Test graceful fallback when Gemini response is not JSON at all
  #[test]
  fn test_parse_non_json_fallback() {
    let raw = "ขออภัย ไม่สามารถอ่านสลิปได้";
    let parsed: serde_json::Value = serde_json::from_str(raw.trim())
      .unwrap_or_else(|_| serde_json::json!({"amount": null, "note": raw}));
    assert!(parsed["amount"].is_null());
    assert_eq!(parsed["note"].as_str(), Some(raw));
  }

  /// Test GeminiRequest serializes correctly per Gemini REST API spec
  #[test]
  fn test_request_serialization() {
    let req = GeminiRequest {
      contents: vec![GeminiContent {
        parts: vec![
          GeminiPart::InlineData {
            inline_data: InlineData {
              mime_type: "image/jpeg".to_string(),
              data: "base64data==".to_string(),
            },
          },
          GeminiPart::Text {
            text: "อ่านสลิปนี้".to_string(),
          },
        ],
      }],
      generation_config: GenerationConfig {
        temperature: 0.0,
        max_output_tokens: 1024,
        response_mime_type: "application/json".to_string(),
        thinking_config: ThinkingConfig { thinking_budget: 0 },
      },
    };

    let json = serde_json::to_value(&req).unwrap();

    // Verify inlineData part
    let inline = &json["contents"][0]["parts"][0]["inlineData"];
    assert_eq!(inline["mimeType"], "image/jpeg");
    assert_eq!(inline["data"], "base64data==");

    // Verify text part
    assert_eq!(json["contents"][0]["parts"][1]["text"], "อ่านสลิปนี้");

    // Verify generationConfig
    assert_eq!(json["generationConfig"]["temperature"], 0.0);
    assert_eq!(json["generationConfig"]["maxOutputTokens"], 1024);
    assert_eq!(
      json["generationConfig"]["thinkingConfig"]["thinkingBudget"],
      0
    );
    assert_eq!(
      json["generationConfig"]["responseMimeType"],
      "application/json"
    );
  }

  /// Test extract_json_object — ```json fence
  #[test]
  fn test_extract_json_fence_with_lang() {
    let input = "```json\n{\"amount\": 100.0, \"note\": \"test\"}\n```";
    let result = extract_json_object(input).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["amount"].as_f64(), Some(100.0));
  }

  /// Test extract_json_object — ``` fence without lang
  #[test]
  fn test_extract_json_fence_no_lang() {
    let input = "```\n{\"amount\": 200.0, \"note\": \"test2\"}\n```";
    let result = extract_json_object(input).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["amount"].as_f64(), Some(200.0));
  }

  /// Test extract_json_object — raw JSON without fence
  #[test]
  fn test_extract_json_no_fence() {
    let input = "{\"amount\": 300.0, \"note\": \"no fence\"}";
    let result = extract_json_object(input).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["amount"].as_f64(), Some(300.0));
  }

  /// Test extract_json_object — JSON embedded in text
  #[test]
  fn test_extract_json_embedded_in_text() {
    let input = "ยอดโอนคือ {\"amount\": 50.0, \"note\": \"embedded\"} ครับ";
    let result = extract_json_object(input).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["amount"].as_f64(), Some(50.0));
  }

  /// Test extract_json_object — no JSON at all, must return None
  #[test]
  fn test_extract_json_no_json() {
    let input = "ขออภัย ไม่สามารถอ่านสลิปได้";
    assert!(extract_json_object(input).is_none());
  }

  /// Test extract_json_object — truncated JSON should fail to parse
  #[test]
  fn test_extract_json_truncated() {
    let input = "{\"amount\": 100.";
    // Extract succeeds but parse fails → should fallback
    let result = extract_json_object(input);
    // No closing } → None
    assert!(result.is_none());
  }

  /// Test round-trip: extract then parse JSON correctly
  #[test]
  fn test_extract_then_parse() {
    let input = "```json\n{\"amount\": 100.0, \"note\": \"โอน 100 บาท\"}\n```";
    let extracted = extract_json_object(input).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&extracted).unwrap();
    assert_eq!(parsed["amount"].as_f64(), Some(100.0));
    assert_eq!(parsed["note"].as_str(), Some("โอน 100 บาท"));
  }

  /// Test GeminiResponse deserializes from JSON returned by API
  #[test]
  fn test_response_deserialization() {
    let raw = r#"{
            "candidates": [{
                "content": {
                    "parts": [{"text": "{\"amount\": 250.0, \"note\": \"โอน 250 บาท\"}"}]
                }
            }]
        }"#;

    let resp: GeminiResponse = serde_json::from_str(raw).unwrap();
    let text = resp
      .candidates
      .into_iter()
      .next()
      .and_then(|c| c.content.parts.into_iter().find_map(|p| p.text))
      .unwrap();

    let parsed: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
    assert_eq!(parsed["amount"].as_f64(), Some(250.0));
    assert_eq!(parsed["note"].as_str(), Some("โอน 250 บาท"));
  }
}
