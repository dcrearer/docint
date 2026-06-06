# Integration Test Setup

## Overview

Integration tests verify Row-Level Security (RLS) tenant isolation works correctly under real database conditions. Each test gets its own isolated PostgreSQL database with pgvector.

## Architecture

### Test Database Strategy
- **One database per test** for complete isolation
- **Superuser creates DB** → **Non-privileged user runs tests**
- RLS policies enforced on test_user (non-superuser)

### Components
1. **Podman container** - PostgreSQL 16 with pgvector (port 5433)
2. **Superuser (postgres)** - Creates databases, runs migrations
3. **Test user (test_user)** - Non-privileged, RLS applies
4. **Unique DB per test** - Format: `docint_test_<uuid>`

## Quick Start

### 1. Start Test Database
```bash
podman-compose -f docker-compose.test.yml up -d
podman-compose -f docker-compose.test.yml ps  # Verify healthy
```

### 2. Run Integration Tests
```bash
# All RLS tests
cargo test --test store_rls_tests -- --ignored

# Specific test
cargo test --test store_rls_tests test_tenant_isolation_basic -- --ignored --exact

# With output
cargo test --test store_rls_tests -- --ignored --nocapture
```

### 3. Stop Test Database
```bash
podman-compose -f docker-compose.test.yml down -v
```

## Test Coverage

### Current Tests (7 total)
✅ `test_tenant_isolation_basic` - Tenant A/B document separation  
✅ `test_tenant_isolation_search` - Search respects RLS  
✅ `test_concurrent_tenant_requests_no_leakage` - Concurrent safety  
✅ `test_transaction_scoped_tenant_context_is_cleared` - Clean connections  
✅ `test_rls_policy_filters_chunks_by_document_tenant` - Chunk-level RLS  
✅ `common::tests::test_setup_test_db` - Database setup works  
✅ `common::tests::test_seed_and_cleanup` - Data seeding works

### What They Verify
- **P0 #1 RLS Fix** - Transaction-scoped `app.tenant_id`
- **No tenant leakage** - Tenant A cannot see Tenant B's data
- **Connection pool safety** - No stale session state
- **Concurrent isolation** - Parallel requests don't interfere
- **RLS enforcement** - Policies work for SELECT/INSERT/UPDATE/DELETE

## How It Works

### Setup (per test)
1. **Generate unique DB name** - `docint_test_a1b2c3d4`
2. **Connect as superuser** - `postgres:postgres@localhost:5433/postgres`
3. **Create database** - `CREATE DATABASE "docint_test_a1b2c3d4"`
4. **Run migrations** - Creates tables, pgvector extension, RLS policies
5. **Grant permissions** - `GRANT ALL PRIVILEGES ON ALL TABLES TO test_user`
6. **Return pool as test_user** - Non-privileged connection

### During Test
- All queries run as `test_user`
- RLS policies enforce tenant isolation
- `with_tenant()` sets `app.tenant_id` per transaction
- Inserts/selects filtered by tenant context

### Teardown
- Database automatically cleaned up (unique per test)
- No manual cleanup needed

## Files

### Test Infrastructure
- `docker-compose.test.yml` - PostgreSQL container definition
- `scripts/init-test-db.sql` - Creates test_user on startup
- `crates/docint-core/tests/common/mod.rs` - Test helpers
- `crates/docint-core/tests/store_rls_tests.rs` - RLS integration tests

### Migrations (applied automatically)
- `20250101000001_initial_schema.sql` - Tables, indexes, pgvector
- `20250101000002_fulltext_search.sql` - Full-text search
- `20250101000003_row_level_security.sql` - Enable RLS
- `20250101000004_unique_document_per_tenant.sql` - Constraints
- `20250101000005_rls_write_policies.sql` - INSERT/UPDATE/DELETE policies

## Troubleshooting

### "Connection refused"
```bash
podman-compose -f docker-compose.test.yml ps  # Check if running
podman-compose -f docker-compose.test.yml up -d  # Start if stopped
```

### "Permission denied"
- Ensure test_user is NOT a superuser
- Check `scripts/init-test-db.sql` doesn't grant SUPERUSER
- Verify RLS is enabled: `ALTER TABLE documents FORCE ROW LEVEL SECURITY`

### "RLS policy violation"
- Queries must use `with_tenant()` wrapper
- Check `app.tenant_id` is set: `SELECT current_setting('app.tenant_id', true)`

### Tests see wrong data
- Each test should create unique database
- Check `setup_test_db()` generates unique name
- Verify no shared state between tests

### Slow tests
- Expected: ~50-100ms per test (DB creation + migrations)
- If >1s per test, check container performance

## Performance

- **Test execution**: ~340ms for 7 tests
- **Per test overhead**: ~50ms (DB creation + migrations)
- **Parallel execution**: Tests run concurrently by default
- **Total databases created**: 7 (one per test)

## Next Steps

See `docs/TEST-COVERAGE-AUDIT.md` for:
- Phase 2: Business logic tests (RRF algorithm, search)
- Phase 3: Lambda handler tests
- Phase 4: Auth module tests
- Target: 75%+ code coverage
