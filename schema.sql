-- ============================================
-- KaoBot Database Schema
-- Run this once in Supabase SQL Editor
-- ============================================

-- Table: expenses
CREATE TABLE IF NOT EXISTS expenses (
    id          BIGSERIAL PRIMARY KEY,
    item        TEXT            NOT NULL,
    amount      NUMERIC(10,2)   NOT NULL,
    paid_by     TEXT            NOT NULL DEFAULT 'wife',
    chat_id     BIGINT          NOT NULL,
    message_id  BIGINT,                         -- for deduplication
    is_cleared  BOOLEAN         NOT NULL DEFAULT FALSE,
    cleared_at  TIMESTAMPTZ,
    created_at  TIMESTAMPTZ     NOT NULL DEFAULT NOW()
);

-- Table: payments
CREATE TABLE IF NOT EXISTS payments (
    id           BIGSERIAL PRIMARY KEY,
    amount       NUMERIC(10,2) NOT NULL,
    method       TEXT          NOT NULL DEFAULT 'slip',  -- 'slip' | 'manual'
    slip_image   TEXT,                                   -- base64 or URL (optional)
    chat_id      BIGINT        NOT NULL,
    message_id   BIGINT,
    note         TEXT,                                   -- e.g. slip OCR notes
    created_at   TIMESTAMPTZ   NOT NULL DEFAULT NOW()
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_expenses_chat_id     ON expenses(chat_id);
CREATE INDEX IF NOT EXISTS idx_expenses_is_cleared  ON expenses(is_cleared);
CREATE INDEX IF NOT EXISTS idx_expenses_created_at  ON expenses(created_at);
CREATE INDEX IF NOT EXISTS idx_payments_chat_id     ON payments(chat_id);

-- View: outstanding balance (used by /summary)
CREATE OR REPLACE VIEW pending_summary AS
SELECT
    chat_id,
    COUNT(*)            AS item_count,
    SUM(amount)         AS total_amount,
    MIN(created_at)     AS oldest_expense,
    MAX(created_at)     AS latest_expense
FROM expenses
WHERE is_cleared = FALSE
GROUP BY chat_id;

-- Table: credit_balance (overpaid amount, auto-applied to future expenses)
CREATE TABLE IF NOT EXISTS credit_balance (
    chat_id     BIGINT        PRIMARY KEY,      -- 1 row per chat
    balance     NUMERIC(10,2) NOT NULL DEFAULT 0.00,
    updated_at  TIMESTAMPTZ   NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_credit_balance_chat_id ON credit_balance(chat_id);

ALTER TABLE credit_balance ENABLE ROW LEVEL SECURITY;
CREATE POLICY "Allow all for anon" ON credit_balance FOR ALL USING (true);

-- View: daily summary
CREATE OR REPLACE VIEW daily_summary AS
SELECT
    chat_id,
    DATE(created_at AT TIME ZONE 'Asia/Bangkok') AS expense_date,
    COUNT(*)        AS item_count,
    SUM(amount)     AS total_amount
FROM expenses
WHERE is_cleared = FALSE
GROUP BY chat_id, DATE(created_at AT TIME ZONE 'Asia/Bangkok')
ORDER BY expense_date DESC;

-- ============================================
-- Row Level Security
-- ============================================
ALTER TABLE expenses ENABLE ROW LEVEL SECURITY;
ALTER TABLE payments ENABLE ROW LEVEL SECURITY;

-- Policy: allow all operations via anon key (suitable for dev; use service_role in production)
CREATE POLICY "Allow all for anon" ON expenses FOR ALL USING (true);
CREATE POLICY "Allow all for anon" ON payments FOR ALL USING (true);
