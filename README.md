# KaoBot

> A Telegram bot for tracking shared household expenses. Built with Rust, Supabase, and Gemini Vision AI.

[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange)](https://www.rust-lang.org/)
[![Edition](https://img.shields.io/badge/edition-2024-blue)](https://doc.rust-lang.org/edition-guide/rust-2024/)
[![CI](https://github.com/your-org/kaobot/actions/workflows/ci.yml/badge.svg)](https://github.com/your-org/kaobot/actions/workflows/ci.yml)

## Features

| Feature | Usage |
|---|---|
| Log expenses | Type `rice 60` or `coffee 65.50` |
| View pending balance | `/summary` |
| Today's items | `/today` |
| Recent history | `/history` |
| Record a transfer | `/paid 500` |
| Auto-read slips | Send a bank slip photo |
| Cancel an item | `/cancel 42` |
| Clear all | `/clear` |

## Quick Start

### 1. Create a Telegram Bot

1. Message [@BotFather](https://t.me/botfather) with `/newbot`
2. Add the bot to your group and make it **Admin** (required to read group messages)

### 2. Set Up Supabase

1. Create a project at [supabase.com](https://supabase.com)
2. Run `schema.sql` in the SQL Editor
3. Copy **Project URL** and **anon public** key from Settings → API

### 3. Get a Gemini API Key

Create a key at [aistudio.google.com/app/apikey](https://aistudio.google.com/app/apikey). Free tier covers 1,500 requests/day.

### 4. Configure

```bash
cp .env.example .env
```

Edit `.env`:

```env
TELOXIDE_TOKEN=<from BotFather>
SUPABASE_URL=<project URL>
SUPABASE_ANON_KEY=<anon key>
GEMINI_API_KEY=<your key>
ALLOWED_CHAT_ID=          # leave empty for now
```

### 5. Find Your Chat ID

```bash
docker compose up --build
```

Send any message in your group, then check logs for:

```
INFO kaobot: Message from chat_id: -1001234567890
```

### 6. Lock to Your Group

Stop the bot (`Ctrl+C`), set `ALLOWED_CHAT_ID` in `.env`, then:

```bash
docker compose up -d --build
```

## Usage

### Log an Expense

Just type the item name and amount separated by a space:

```
rice 60
coffee 65.50
household supplies 320
```

Multi-word names and Thai text are supported. Amounts must be > 0 and ≤ 1,000,000. Messages that don't match this format are silently ignored.

### Record a Payment

```
/paid 500
```

Clears all pending items. If you overpay, the excess is stored as credit and auto-applied to future expenses.

### Send a Slip

Send a bank transfer screenshot to the group. Gemini reads the amount and settles automatically. If the amount can't be read, use `/paid <amount>` as a fallback.

### Commands

| Command | Description |
|---|---|
| `/help` | Show all commands |
| `/summary` | All pending items with total |
| `/today` | Today's items (Asia/Bangkok time) |
| `/history` | Last 10 pending items |
| `/paid <amount>` | Record a transfer and clear all |
| `/cancel <id>` | Remove a specific item |
| `/clear` | Clear all (no payment record) |

## Project Structure

```
kaobot/
├── src/
│   ├── main.rs         # Entry point, config, message routing
│   ├── commands.rs     # Command handlers (/summary, /paid, /cancel, …)
│   ├── parser.rs       # Parse "item amount" text format
│   ├── supabase.rs     # Supabase REST API client
│   └── slip.rs         # Gemini Vision API — read bank slip images
├── schema.sql          # Supabase schema (run once)
├── Cargo.toml
├── Dockerfile          # Multi-stage build
├── docker-compose.yml
└── .env.example
```

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        Telegram Chat                        │
└──────────────────────┬──────────────────┬───────────────────┘
                       │ text message     │ photo (slip)
                       ▼                  ▼
              ┌────────────────┐  ┌────────────────────┐
              │  parser.rs     │  │   slip.rs           │
              │ parse_expense()│  │ Gemini Vision API   │
              │ "rice 60"      │  │ → amount: 500       │
              └───────┬────────┘  └────────┬────────────┘
                      │                    │
                      ▼                    ▼
              ┌────────────────────────────────────┐
              │           supabase.rs              │
              │  insert_expense()                  │
              │  insert_payment() + clear_all()    │
              │  get_pending_total()               │
              └────────────────────────────────────┘
                              │
                              ▼
                    ┌─────────────────┐
                    │  Supabase DB    │
                    │  (PostgreSQL)   │
                    │  expenses       │
                    │  payments       │
                    │  pending_summary│
                    └─────────────────┘
```

## Docker

```bash
# Start
docker compose up -d --build

# View logs
docker compose logs -f

# Stop
docker compose down

# Rebuild after config change
docker compose down && docker compose up -d --build
```

## Running Natively

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
cargo run
cargo build --release
```

## Development

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt
```

## Database Schema

Run `schema.sql` once in Supabase SQL Editor. It creates:

| Object | Type | Purpose |
|---|---|---|
| `expenses` | table | Individual expense items |
| `payments` | table | Transfer/payment records |
| `credit_balance` | table | Overpayment credit per chat |
| `pending_summary` | view | Pending totals grouped by chat |
| `daily_summary` | view | Daily expense summary |

## Troubleshooting

**Bot doesn't respond in group**
- Make sure the bot is an **Admin** in the group
- Verify `TELOXIDE_TOKEN` and `ALLOWED_CHAT_ID` in `.env`
- Check logs: `docker compose logs -f`

**"SUPABASE_URL must be set"**
- Ensure `.env` is in the same directory as `docker-compose.yml`
- No spaces around `=` in `.env`

**Slip reading fails**
- Verify `GEMINI_API_KEY` is valid and has quota remaining
- Use a direct screenshot from your banking app (not a photo of a screen)
- Fall back to `/paid <amount>`

**Expense saves but bot replies with error**
- Confirm `schema.sql` was run in Supabase
- Check RLS policies: both `expenses` and `payments` tables need a permissive policy for the anon key

## Notes

- Uses **Long Polling** — no public IP or webhook needed
- Slip images are sent to Gemini temporarily and never stored
- Each `chat_id` has its own isolated ledger
- `ALLOWED_CHAT_ID` is optional but recommended

## License

MIT
