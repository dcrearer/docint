-- Enable row-level security on documents and chunks.
-- Queries must SET app.tenant_id = 'xxx' before accessing data.
-- Without it, no rows are visible (deny by default).

ALTER TABLE documents ENABLE ROW LEVEL SECURITY;
ALTER TABLE chunks ENABLE ROW LEVEL SECURITY;

-- Policy: users can only see documents matching their tenant_id
CREATE POLICY tenant_isolation_documents ON documents
    USING (tenant_id = current_setting('app.tenant_id', true));

-- Policy: users can only see chunks belonging to their tenant's documents
CREATE POLICY tenant_isolation_chunks ON chunks
    USING (document_id IN (
        SELECT id FROM documents WHERE tenant_id = current_setting('app.tenant_id', true)
    ));

-- The docint user (used by Lambdas) is NOT a superuser,
-- so RLS applies. Superusers bypass RLS by default.
-- Force RLS even for table owner:
ALTER TABLE documents FORCE ROW LEVEL SECURITY;
ALTER TABLE chunks FORCE ROW LEVEL SECURITY;
