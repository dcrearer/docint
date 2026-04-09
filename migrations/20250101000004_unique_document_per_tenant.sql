-- Prevent duplicate documents per tenant. Re-ingesting the same S3 key
-- should update the existing document, not create a new one.
-- First, remove duplicates keeping only the newest per (tenant_id, source_key).
DELETE FROM documents d
WHERE d.id NOT IN (
    SELECT DISTINCT ON (tenant_id, source_key) id
    FROM documents
    ORDER BY tenant_id, source_key, created_at DESC
);

CREATE UNIQUE INDEX idx_documents_tenant_key ON documents (tenant_id, source_key);
