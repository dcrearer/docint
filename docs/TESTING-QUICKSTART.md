# Testing Quick Start Guide

## Setup Test Environment

### 1. Start Test Database

```bash
# Start PostgreSQL with pgvector
docker-compose -f docker-compose.test.yml up -d

# Wait for database to be healthy
docker-compose -f docker-compose.test.yml ps

# Verify connection
psql -h localhost -p 5433 -U test_user -d docint_test -c "SELECT version();"
```

### 2. Run Migrations

```bash
# Migrations run automatically in tests, but you can run manually:
DATABASE_URL="postgres://test_user:test_pass@localhost:5433/docint_test" \
  sqlx migrate run
```

---

## Running Tests

### Unit Tests (Fast, no database)

```bash
# Run all unit tests
cargo test --lib

# Run specific module
cargo test --lib chunker::tests

# Run with output
cargo test --lib -- --nocapture
```

**Expected output:**
```
running 4 tests
test chunker::tests::empty_text ... ok
test chunker::tests::single_sentence ... ok
test chunker::tests::basic_chunking ... ok
test chunker::tests::overlap_works ... ok

test result: ok. 4 passed
```

---

### Integration Tests (Require database)

```bash
# Run all integration tests (requires test DB)
cargo test --test '*' -- --ignored

# Run specific test file
cargo test --test store_rls_tests -- --ignored

# Run specific test
cargo test --test store_rls_tests test_tenant_isolation_basic -- --ignored
```

**Expected output:**
```
running 5 tests
test test_tenant_isolation_basic ... ok
test test_tenant_isolation_search ... ok
test test_concurrent_tenant_requests_no_leakage ... ok
test test_transaction_scoped_tenant_context_is_cleared ... ok
test test_rls_policy_filters_chunks_by_document_tenant ... ok

test result: ok. 5 passed
```

---

### All Tests

```bash
# Run everything (unit + integration)
cargo test --workspace -- --ignored

# Run without ignored tests (just unit tests)
cargo test --workspace
```

---

## TDD Workflow Example

### Red-Green-Refactor Cycle

```bash
# 1. Write a failing test (RED)
cat > crates/docint-core/tests/new_feature_test.rs <<'EOF'
#[tokio::test]
#[ignore]
async fn test_new_feature() {
    let result = my_new_function();
    assert_eq!(result, "expected");
}
EOF

# 2. Run test - it should fail
cargo test --test new_feature_test -- --ignored
# ❌ Error: cannot find function `my_new_function`

# 3. Implement the function (GREEN)
# ... edit source code ...

# 4. Run test - it should pass
cargo test --test new_feature_test -- --ignored
# ✅ test test_new_feature ... ok

# 5. Refactor and commit
git add -A
git commit -m "feat: add new_feature with test coverage"
```

---

## Test Coverage

### Generate Coverage Report

```bash
# Install tarpaulin (once)
cargo install cargo-tarpaulin

# Generate HTML coverage report
cargo tarpaulin --out Html --output-dir coverage

# Open report
open coverage/index.html
```

### Coverage Targets

- **Phase 1:** 80% coverage for `store.rs` and `db.rs`
- **Phase 2:** 75% overall coverage
- **Phase 3:** 85% for critical modules

---

## Writing New Tests

### Test Structure

```rust
//! Module-level documentation

mod common; // Import test helpers

use common::{setup_test_db, seed_test_data};
use docint_core::store::VectorStore;

#[tokio::test]
#[ignore] // Mark as integration test (requires DB)
async fn test_my_feature() {
    // GIVEN: Setup test state
    let pool = setup_test_db().await.unwrap();
    seed_test_data(&pool, "tenant-123", "Test").await.unwrap();

    // WHEN: Execute the feature
    let result = my_function(&pool).await.unwrap();

    // THEN: Assert expected outcome
    assert_eq!(result, expected_value);
}
```

### Test Naming Convention

- `test_<feature>_<scenario>` - Describes what and when
- Examples:
  - `test_tenant_isolation_basic`
  - `test_search_handles_empty_query`
  - `test_insert_document_upsert_on_conflict`

### Assertions

```rust
// Equality
assert_eq!(actual, expected);
assert_ne!(actual, unexpected);

// Boolean
assert!(condition, "optional message");

// Result/Option
assert!(result.is_ok());
assert!(result.is_err());
assert!(option.is_some());

// Custom assertions
assert!(
    docs.iter().all(|d| d.tenant_id == "tenant-123"),
    "All documents should belong to tenant-123"
);
```

---

## Debugging Tests

### Run Single Test with Output

```bash
# Show println! output
cargo test test_name -- --nocapture

# Show test logs
RUST_LOG=debug cargo test test_name -- --nocapture
```

### Connect to Test Database

```bash
# Connect with psql
psql -h localhost -p 5433 -U test_user -d docint_test

# Check tenant isolation
docint_test=# SELECT current_setting('app.tenant_id', true);
docint_test=# SELECT * FROM documents;
```

### Reset Test Database

```bash
# Drop and recreate
docker-compose -f docker-compose.test.yml down -v
docker-compose -f docker-compose.test.yml up -d

# Or truncate tables
psql -h localhost -p 5433 -U test_user -d docint_test \
  -c "TRUNCATE TABLE chunks, documents RESTART IDENTITY CASCADE;"
```

---

## CI Integration

The tests run automatically in GitHub Actions:

```yaml
# .github/workflows/tests.yml
- name: Start test database
  run: docker-compose -f docker-compose.test.yml up -d

- name: Run unit tests
  run: cargo test --workspace

- name: Run integration tests
  run: cargo test --workspace --test '*' -- --ignored

- name: Generate coverage
  run: cargo tarpaulin --out Xml
```

---

## Common Issues

### Issue: "Connection refused"
**Cause:** Test database not running  
**Fix:**
```bash
docker-compose -f docker-compose.test.yml up -d
docker-compose -f docker-compose.test.yml ps # verify running
```

### Issue: "relation does not exist"
**Cause:** Migrations not run  
**Fix:**
```bash
DATABASE_URL="postgres://test_user:test_pass@localhost:5433/docint_test" \
  sqlx migrate run
```

### Issue: "Tenant isolation test fails"
**Cause:** Stale test data from previous run  
**Fix:**
```bash
psql -h localhost -p 5433 -U test_user -d docint_test \
  -c "TRUNCATE TABLE chunks, documents RESTART IDENTITY CASCADE;"
```

### Issue: "Too many connections"
**Cause:** Connection pool not closed between tests  
**Fix:** Use `#[serial]` from `serial_test` crate or increase pool limits

---

## Next Steps

1. ✅ **Setup complete** - Test infrastructure is ready
2. 🔄 **Phase 1** - Write RLS tenant isolation tests (already started)
3. 📝 **Phase 2** - Add store.rs business logic tests
4. 🚀 **Phase 3** - Lambda handler integration tests
5. 📊 **Phase 4** - Coverage reports and CI integration

**Current status:** Ready to begin TDD!

**Next command:**
```bash
# Start test database and run existing tests
docker-compose -f docker-compose.test.yml up -d
cargo test --test store_rls_tests -- --ignored
```
