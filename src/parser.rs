// parser.rs — Parse "name amount" from text

use regex::Regex;
use std::sync::OnceLock;

static EXPENSE_RE: OnceLock<Regex> = OnceLock::new();

fn expense_regex() -> &'static Regex {
  EXPENSE_RE.get_or_init(|| {
    // Pattern: any text + space + number (supports decimals)
    // Examples: "rice 60", "coffee 65.50", "household items 1200"
    Regex::new(r"^(.+?)\s+(\d+(?:\.\d{1,2})?)$").unwrap()
  })
}

/// Parse text into (item_name, amount)
/// Returns None if format doesn't match
pub fn parse_expense(text: &str) -> Option<(String, f64)> {
  let text = text.trim();
  let re = expense_regex();

  let caps = re.captures(text)?;
  let item = caps[1].trim().to_string();
  let amount: f64 = caps[2].parse().ok()?;

  if item.is_empty() || amount <= 0.0 || amount > 1_000_000.0 {
    return None;
  }

  Some((item, amount))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_basic_expense() {
    assert_eq!(parse_expense("ข้าว 60"), Some(("ข้าว".to_string(), 60.0)));
  }

  #[test]
  fn test_decimal_amount() {
    assert_eq!(
      parse_expense("กาแฟ 65.50"),
      Some(("กาแฟ".to_string(), 65.5))
    );
  }

  #[test]
  fn test_multi_word_item() {
    assert_eq!(
      parse_expense("ของใช้ในบ้าน 1200"),
      Some(("ของใช้ในบ้าน".to_string(), 1200.0))
    );
  }

  #[test]
  fn test_invalid_format() {
    assert_eq!(parse_expense("สวัสดี"), None);
    assert_eq!(parse_expense("60"), None);
    assert_eq!(parse_expense("/help"), None);
  }

  #[test]
  fn test_with_spaces() {
    assert_eq!(
      parse_expense("  ข้าวมันไก่  120  "),
      Some(("ข้าวมันไก่".to_string(), 120.0))
    );
  }
}
