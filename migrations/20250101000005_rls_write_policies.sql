-- Add INSERT/UPDATE/DELETE policies for RLS
-- The existing policies only cover SELECT (USING clause)
-- We need WITH CHECK clauses for write operations

-- Drop existing policies and recreate with full CRUD support
DROP POLICY IF EXISTS tenant_isolation_documents ON documents;
DROP POLICY IF EXISTS tenant_isolation_chunks ON chunks;

-- Documents: full CRUD with tenant isolation
CREATE POLICY tenant_isolation_documents ON documents
    FOR ALL
    USING (tenant_id = current_setting('app.tenant_id', true))
    WITH CHECK (tenant_id = current_setting('app.tenant_id', true));

-- Chunks: full CRUD, filtered by parent document's tenant
CREATE POLICY tenant_isolation_chunks ON chunks
    FOR ALL
    USING (document_id IN (
        SELECT id FROM documents WHERE tenant_id = current_setting('app.tenant_id', true)
    ))
    WITH CHECK (document_id IN (
        SELECT id FROM documents WHERE tenant_id = current_setting('app.tenant_id', true)
    ));
