# Lambda Compare Handler Integration Tests

This directory contains integration tests for the lambda-compare handler.

## Prerequisites

- PostgreSQL test database running on `localhost:5433`
- Test user `test_user` with password `test_pass` configured
- Migrations applied (handled automatically by test setup)

## Running Tests

Run all integration tests:
```bash
cargo test --package lambda-compare --test handler_tests -- --ignored
```

Run a specific test:
```bash
cargo test --package lambda-compare --test handler_tests test_handler_compares_two_documents -- --ignored
```

## Test Coverage

### `test_handler_compares_two_documents`
Validates that the handler successfully compares two documents and returns results from both, with document-specific content preserved.

### `test_handler_searches_within_each_document`
Verifies that search results are strictly scoped to the requested documents - no cross-document contamination, and documents not in the comparison are excluded.

### `test_handler_requires_both_document_ids`
Tests validation behavior when one document ID is invalid/missing. The handler should return empty results for the missing document.

### `test_handler_enforces_tenant_isolation`
Critical security test: Ensures RLS (Row Level Security) prevents cross-tenant data access. Tenant A cannot see Tenant B's documents and vice versa.

## Test Database

Each test gets a unique PostgreSQL database to ensure complete isolation. Databases are automatically created and torn down by the test framework.

## Common Test Utilities

Tests use shared helpers from `docint-core/tests/common/mod.rs`:
- `setup_test_db()` - Creates isolated test database
- `seed_test_data()` - Seeds standard test data
- Database lifecycle management

## Notes

- All tests are marked with `#[ignore]` to prevent running without a test database
- Tests use simulated handler logic (inline version) to avoid Lambda runtime complexity
- Mock embeddings are used instead of calling Bedrock API
- RLS policies are tested through the `with_tenant()` transaction wrapper
