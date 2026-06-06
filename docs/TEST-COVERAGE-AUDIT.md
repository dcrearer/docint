# Test Coverage Audit & TDD Implementation Plan

**Date:** 2026-06-05  
**Current Status:** 9 tests total, covering 2 modules only  
**Target:** Comprehensive TDD coverage for all critical business logic

---

## Current Test Coverage Summary

| Module | Tests | Coverage | Status |
|--------|-------|----------|--------|
| **docint-core/chunker** | 4 tests | ~80% | ✅ Good |
| **lambda-ingest (helpers)** | 5 tests | ~60% | ⚠️ Partial |
| **docint-core/db** | 0 tests | 0% | ❌ **CRITICAL** |
| **docint-core/store** | 0 tests | 0% | ❌ **CRITICAL** |
| **docint-core/embeddings** | 0 tests | 0% | ❌ Missing |
| **docint-core/models** | 0 tests | 0% | ❌ Missing |
| **docint-cli/auth** | 0 tests | 0% | ❌ Missing |
| **lambda-search** | 0 tests | 0% | ❌ Missing |
| **lambda-metadata** | 0 tests | 0% | ❌ Missing |
| **lambda-compare** | 0 tests | 0% | ❌ Missing |

**Total:** 9 tests across 59 source files = **15% file coverage**

---

## Critical Gaps (From PRIORITY-ISSUES.md)

### **P0 - CRITICAL (Zero Tests)**

#### 1. **`store.rs` - Search, RRF, Insert** ⚠️ HIGHEST PRIORITY
**Lines of Code:** ~210  
**Tests:** 0  
**Risk:** This is the MOST CRITICAL module - handles all database operations

**What needs testing:**
- `insert_document_tx()` - Document creation/update logic
- `insert_chunk_tx()` - Chunk insertion
- `hybrid_search_tx()` - RRF (Reciprocal Rank Fusion) algorithm
- `similarity_search_tx()` - Vector search
- `search_within_document_tx()` - Document-scoped search
- `get_metadata_tx()` - Metadata retrieval
- `list_documents_tx()` - Document listing
- **RLS tenant isolation** - Ensure tenant A can't see tenant B's data
- Transaction handling - Verify `with_tenant()` works correctly

**Why this is critical:**
- Core business logic for search
- RRF scoring algorithm is complex and untested
- Tenant isolation security depends on this working correctly
- Recent P0 fix (transaction-scoped RLS) is unverified

#### 2. **`db.rs` - Tenant Context & Connection Pool**
**Lines of Code:** ~50  
**Tests:** 0  
**Risk:** HIGH - Security-critical (P0 #1 fix)

**What needs testing:**
- `with_tenant()` - Transaction-scoped tenant context
- Verify `set_config(..., true)` is transaction-scoped
- Connection pool cleanup (no stale session state)
- Concurrent request handling with shared pool
- **Security:** Tenant isolation under concurrent load

**Why this is critical:**
- P0 #1 fix (RLS tenant isolation) is UNTESTED
- Security vulnerability if tenant context leaks
- Concurrency issues could cause data leaks

---

### **P1 - HIGH (Zero Tests)**

#### 3. **`auth.rs` - Token Management**
**Lines of Code:** ~150  
**Tests:** 0  
**Risk:** MEDIUM-HIGH - Authentication security

**What needs testing:**
- Token refresh logic
- JWT parsing and validation
- Token expiry handling
- Cache read/write
- Error handling for invalid tokens

#### 4. **`embeddings.rs` - Bedrock Integration**
**Lines of Code:** ~80  
**Tests:** 0  
**Risk:** MEDIUM - Integration with AWS Bedrock

**What needs testing:**
- Embedding generation (mocked Bedrock calls)
- Error handling for Bedrock failures
- Input validation
- Vector serialization/deserialization

#### 5. **Lambda Handlers**
**Lines of Code:** ~400 (4 handlers)  
**Tests:** 0  
**Risk:** MEDIUM - Request handling logic

**What needs testing:**
- Request parsing
- Response formatting
- Error handling
- Tenant validation
- Input sanitization

---

## Existing Test Coverage

### ✅ **chunker.rs** (4 tests - Good coverage)
```rust
✅ test chunker::tests::empty_text
✅ test chunker::tests::single_sentence  
✅ test chunker::tests::basic_chunking
✅ test chunker::tests::overlap_works
```

**What's tested:**
- Empty input handling
- Single sentence chunking
- Multi-sentence chunking with overlap
- Chunk size limits

**What's missing:**
- Edge cases: very long sentences (>8K chars)
- Unicode handling
- CSV/log format edge cases

---

### ⚠️ **lambda-ingest helpers** (5 tests - Partial coverage)
```rust
✅ test tests::tenant_from_key_uuid_prefix
✅ test tests::tenant_from_key_legacy_prefix
✅ test tests::tenant_from_key_bare_file_falls_back
✅ test tests::tenant_from_key_no_prefix_falls_back
✅ test tests::title_from_key_strips_extension
```

**What's tested:**
- S3 key parsing for tenant extraction
- Title extraction from filenames

**What's missing:**
- Main ingestion flow (`ingest_file()`)
- Error handling
- Batch embedding generation
- Transaction rollback on failure

---

## TDD Implementation Plan

### **Phase 1: Critical Security Tests (Week 1)**

#### Priority 1A: `store.rs` - RLS Tenant Isolation
**Goal:** Verify P0 #1 fix works correctly

```rust
// Test file: crates/docint-core/tests/store_rls_tests.rs

#[tokio::test]
async fn test_tenant_isolation_basic() {
    // GIVEN: Two tenants with documents
    // WHEN: Tenant A searches
    // THEN: Only tenant A's documents returned
}

#[tokio::test]
async fn test_tenant_isolation_concurrent_requests() {
    // GIVEN: Connection pool with 1 connection
    // WHEN: Tenant A and B make concurrent requests
    // THEN: Each sees only their own data (no leakage)
}

#[tokio::test]
async fn test_transaction_scoped_tenant_context() {
    // GIVEN: with_tenant() sets app.tenant_id
    // WHEN: Transaction commits
    // THEN: Connection returns to pool clean (no stale tenant_id)
}

#[tokio::test]
async fn test_search_respects_rls_policy() {
    // GIVEN: Tenant A and B both have doc titled "test.txt"
    // WHEN: Tenant A searches for "test"
    // THEN: Only tenant A's doc returned (RLS filters by tenant_id)
}
```

**Estimated time:** 8-12 hours  
**Dependencies:** Test database, Docker Compose for Postgres

---

#### Priority 1B: `db.rs` - Connection Pool & Transactions
**Goal:** Verify transaction-scoped settings work

```rust
// Test file: crates/docint-core/tests/db_tests.rs

#[tokio::test]
async fn test_with_tenant_sets_context() {
    // Verify set_config is called with true (transaction-scoped)
}

#[tokio::test]
async fn test_with_tenant_clears_context_on_commit() {
    // Verify app.tenant_id is cleared after COMMIT
}

#[tokio::test]
async fn test_with_tenant_clears_context_on_error() {
    // Verify app.tenant_id is cleared after ROLLBACK
}

#[tokio::test]
async fn test_connection_pool_reuse_is_clean() {
    // Verify no stale session state persists
}
```

**Estimated time:** 4-6 hours

---

### **Phase 2: Core Business Logic Tests (Week 2)**

#### Priority 2A: `store.rs` - Search & RRF
```rust
#[tokio::test]
async fn test_hybrid_search_rrf_scoring() {
    // Verify Reciprocal Rank Fusion algorithm
    // Given: Vector hits [A, B, C] and FTS hits [B, D, A]
    // When: RRF combines them
    // Then: Order is [B, A, C, D] (B appears in both)
}

#[tokio::test]
async fn test_similarity_search_vector_distance() {
    // Verify cosine similarity ordering
}

#[tokio::test]
async fn test_search_within_document() {
    // Verify document-scoped search
}

#[tokio::test]
async fn test_insert_document_upsert_logic() {
    // Verify ON CONFLICT DO UPDATE works
    // Verify old chunks are deleted
}
```

**Estimated time:** 12-16 hours

---

#### Priority 2B: `embeddings.rs` - Bedrock Integration
```rust
#[tokio::test]
async fn test_embed_text_success() {
    // Mock Bedrock API response
    // Verify vector dimensions (1024 for Titan)
}

#[tokio::test]
async fn test_embed_text_handles_bedrock_error() {
    // Mock Bedrock throttling
    // Verify error handling
}

#[tokio::test]
async fn test_embed_text_validates_input_length() {
    // Verify input length limits
}
```

**Estimated time:** 6-8 hours

---

### **Phase 3: Lambda Handler Tests (Week 3)**

#### Priority 3A: Integration Tests
```rust
// Test file: crates/lambda-search/tests/integration_tests.rs

#[tokio::test]
async fn test_search_handler_success() {
    // GIVEN: Test database with documents
    // WHEN: Valid search request
    // THEN: Returns search results
}

#[tokio::test]
async fn test_search_handler_tenant_isolation() {
    // Verify handler enforces tenant isolation
}

#[tokio::test]
async fn test_search_handler_invalid_tenant_id() {
    // Verify UUID validation (P1 #5)
}
```

**Estimated time:** 10-12 hours (all 4 Lambda handlers)

---

### **Phase 4: Auth & CLI Tests (Week 4)**

#### Priority 4A: `auth.rs`
```rust
#[tokio::test]
async fn test_token_refresh_success() {
    // Mock Cognito token refresh
}

#[tokio::test]
async fn test_token_expiry_handling() {
    // Verify expired tokens are refreshed
}

#[tokio::test]
async fn test_cache_read_write() {
    // Verify token cache works
}
```

**Estimated time:** 8-10 hours

---

## Test Infrastructure Requirements

### **1. Test Database Setup**
```yaml
# docker-compose.test.yml
services:
  test-db:
    image: pgvector/pgvector:pg16
    environment:
      POSTGRES_DB: docint_test
      POSTGRES_USER: test_user
      POSTGRES_PASSWORD: test_pass
    ports:
      - "5433:5432"
```

### **2. Test Fixtures & Helpers**
```rust
// tests/common/mod.rs
pub async fn setup_test_db() -> PgPool { ... }
pub async fn seed_test_data(pool: &PgPool, tenant: &str) { ... }
pub fn mock_embedder() -> Embedder { ... }
```

### **3. CI Integration**
```yaml
# .github/workflows/tests.yml
- name: Run unit tests
  run: cargo test --workspace

- name: Run integration tests
  run: cargo test --workspace --test '*'

- name: Check test coverage
  run: cargo tarpaulin --out Xml
```

---

## Testing Strategy Recommendations

### **Unit Tests (70% of tests)**
- Pure logic (no I/O)
- Mocked dependencies
- Fast execution (<1s per test)
- Run on every commit

**Examples:**
- RRF scoring algorithm
- Tenant ID extraction from S3 keys
- Chunk overlap logic

### **Integration Tests (25% of tests)**
- Real database (Docker Compose)
- Mock AWS services (LocalStack or mocked SDK)
- Medium execution (1-5s per test)
- Run on PR

**Examples:**
- Full search flow (embed + query + RLS)
- Lambda handler end-to-end
- Transaction isolation

### **Property-Based Tests (5% of tests)**
- Using `proptest` or `quickcheck`
- Generate random inputs
- Verify invariants hold

**Examples:**
- Chunking never loses data
- RLS always filters by tenant
- Search results always ordered by score

---

## TDD Workflow

### **Red-Green-Refactor Cycle**

```
1. Write failing test (RED)
   ├─ Test describes desired behavior
   └─ Test fails (function doesn't exist yet)

2. Make test pass (GREEN)
   ├─ Implement minimal code to pass test
   └─ Test passes

3. Refactor (REFACTOR)
   ├─ Clean up implementation
   ├─ Test still passes
   └─ Commit
```

### **Example: Adding Tenant Validation (P1 #5)**

```rust
// Step 1: Write failing test (RED)
#[test]
fn test_validate_tenant_id_accepts_uuid() {
    assert!(validate_tenant_id("a1b2c3d4-e5f6-7890-abcd-ef1234567890").is_ok());
}

#[test]
fn test_validate_tenant_id_rejects_non_uuid() {
    assert!(validate_tenant_id("not-a-uuid").is_err());
}

// Step 2: Implement (GREEN)
pub fn validate_tenant_id(id: &str) -> Result<&str> {
    Uuid::parse_str(id)?;
    Ok(id)
}

// Step 3: Refactor
// (Code is already clean, commit)
```

---

## Success Metrics

| Phase | Target Coverage | Est. Time | Priority |
|-------|----------------|-----------|----------|
| Phase 1: Security | store.rs: 80%, db.rs: 90% | 16h | P0 |
| Phase 2: Business Logic | store.rs: 95%, embeddings: 80% | 20h | P1 |
| Phase 3: Handlers | All handlers: 70% | 12h | P1 |
| Phase 4: Auth & CLI | auth.rs: 80%, cli: 60% | 10h | P2 |
| **Total** | **Overall: 75%+** | **~60h** | - |

---

## Next Steps

1. **Set up test infrastructure** (2-3 hours)
   - Docker Compose for test database
   - Test fixtures and helpers
   - CI integration

2. **Start with Phase 1A** (store.rs RLS tests)
   - Highest priority: verify P0 #1 fix works
   - Tests the most critical security logic

3. **Iterate with TDD**
   - Red-Green-Refactor for each feature
   - Build up comprehensive test suite

4. **Add test coverage reporting**
   - Use `cargo tarpaulin` for coverage metrics
   - Target: 75%+ line coverage

---

## Recommended Tools

- **Test framework:** Built-in `#[test]` + `tokio::test`
- **Assertions:** `assert_eq!`, `anyhow::Result` for errors
- **Mocking:** `mockall` for trait mocking
- **Property testing:** `proptest` for fuzzing
- **Coverage:** `cargo-tarpaulin` for HTML reports
- **Test DB:** Docker Compose with `pgvector/pgvector`

---

**Status:** Ready to begin TDD implementation  
**Next Action:** Set up test infrastructure (Docker Compose)  
**Expected Outcome:** 75%+ test coverage in 4 weeks
