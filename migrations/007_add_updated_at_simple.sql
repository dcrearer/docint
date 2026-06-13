-- Simple version for RDS Data API (no functions/triggers)
-- Add updated_at column with default value
ALTER TABLE documents ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT now();
