-- Add updated_at column to documents table
ALTER TABLE documents
ADD COLUMN updated_at TIMESTAMPTZ NOT NULL DEFAULT now();

-- Backfill: Set updated_at = created_at for existing documents
UPDATE documents SET updated_at = created_at;

-- Add trigger to auto-update updated_at
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER update_documents_updated_at
    BEFORE UPDATE ON documents
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();
