# Docint — Priority Issues

## P0 — Critical (Fix Before Production)

### 1. RLS Tenant Isolation Broken Under Connection Pooling

**Files:** `crates/docint-core/src/db.rs:14-19`, `crates/docint-core/src/store.rs:25-28`

`set_config('app.tenant_id', $1, false)` persists for the **session** (connection), not the transaction. With a connection pool (`max_connections=5`), a connection used for tenant A can be reused for tenant B. If `set_tenant` fails or runs on a different connection than the query, **tenant B sees tenant A's data**.

**Fix:** Use `SET LOCAL app.tenant_id = $1` inside an explicit transaction, or `set_config(..., true)` (transaction-scoped). Wrap all tenant-scoped operations in a transaction:

```rust
pub async fn with_tenant<F, T>(pool: &PgPool, tenant_id: &str, f: F) -> Result<T>
where
    F: FnOnce(&mut Transaction<'_, Postgres>) -> BoxFuture<'_, Result<T>>,
{
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant_id)
        .execute(&mut *tx)
        .await?;
    let result = f(&mut tx).await?;
    tx.commit().await?;
    Ok(result)
}
```

---

### 2. DATABASE_URL Contains Plaintext Credentials

**File:** `infrastructure/stacks/lambda_stack.py:38-39`

CloudFormation `{{resolve:secretsmanager:...}}` dynamic references are resolved at deploy time and stored **in plaintext** in Lambda environment variables. Anyone with `lambda:GetFunctionConfiguration` can read the full connection string including password.

**Fix:** Pass the secret ARN as an env var and resolve it at runtime using the AWS SDK. The Lambdas already have `grant_read` on the secret.

---

### 3. GitHub Deploy Role Has AdministratorAccess

**File:** `infrastructure/bootstrap_github_oidc.py:60-62`

The OIDC condition uses `repo:dcrearer/docint:*` (all branches). A compromised workflow on any branch gets full AWS account access.

**Fix:**
- Restrict to `repo:dcrearer/docint:ref:refs/heads/main`
- Replace `AdministratorAccess` with a scoped custom policy covering only CloudFormation, Lambda, RDS, S3, ECR, IAM:PassRole, and Bedrock

---

### 4. Shared IAM Role Across All Lambdas

**File:** `infrastructure/stacks/lambda_stack.py:14-26`

All four Lambdas share one IAM role. The ingest Lambda needs `s3:GetObject` but search/metadata/compare don't. The search Lambda needs `bedrock:InvokeModel` but metadata doesn't. Violates least privilege.

**Fix:** Create per-Lambda roles (or at minimum separate ingest from query Lambdas).

---

## P1 — High (Security Gaps)

### 5. No Input Validation on `tenant_id`

**Files:** All Lambda handlers

`tenant_id` comes directly from the request payload with no UUID format validation. A prompt injection against the agent could manipulate the `tenant_id` in tool calls.

**Fix:** Add validation in `docint-core`:

```rust
pub fn validate_tenant_id(id: &str) -> Result<&str> {
    Uuid::parse_str(id).context("tenant_id must be a valid UUID")?;
    Ok(id)
}
```

---

### 6. Token Cache Stored in Plaintext

**File:** `crates/docint-cli/src/auth.rs:22-27`

`~/.docint/tokens.json` stores `id_token` and `refresh_token` in plaintext with no file permission restrictions. Refresh tokens are long-lived.

**Fix (minimum):** Set `0600` permissions on the cache file. Ideally use the OS keychain via the `keyring` crate.

---

### 7. S3 Bucket Missing Hardening

**File:** `infrastructure/stacks/lambda_stack.py:68-71`

Missing `enforce_ssl`, explicit `block_public_access`, and lifecycle rules. Storage grows unbounded.

**Fix:** Add `block_public_access=s3.BlockPublicAccess.BLOCK_ALL`, `enforce_ssl=True`, and a lifecycle rule.

---

### 8. Agent Role Has Wildcard Bedrock/Gateway Access

**File:** `infrastructure/stacks/agent_stack.py:24-35`

Grants `foundation-model/*` (any model, including expensive ones) and `bedrock-agentcore:Invoke*` on `*` (any gateway).

**Fix:** Scope to the specific model ID and gateway ARN.

---

## P2 — Medium (Operational Gaps)

### 9. Missing CloudWatch Logs VPC Endpoint

**File:** `infrastructure/stacks/database_stack.py:24-33`

Lambdas in isolated subnets need a VPC endpoint for CloudWatch Logs. Without it, logs may fail silently.

**Fix:** Add `ec2.InterfaceVpcEndpointAwsService.CLOUDWATCH_LOGS` to the VPC endpoints.

---

### 10. No Dead Letter Queue for Ingest Lambda

**File:** `infrastructure/stacks/lambda_stack.py:63-67`

Failed S3 event ingestions are retried twice then **lost forever**.

**Fix:** Add an SQS DLQ via `configure_async_invoke`.

---

### 11. SNS Alarm Topic Has No Subscriptions

**File:** `infrastructure/stacks/monitoring_stack.py:14`

Alarms fire into the void — no email, Slack, or PagerDuty integration.

---

### 12. No Aurora Database Alarms

**File:** `infrastructure/stacks/monitoring_stack.py`

Monitoring only covers Lambda metrics. Missing alarms for CPU, connections, and ACU utilization.

---

### 13. No `limit` Bounds Checking on Lambda Handlers

**Files:** All Lambda handlers

User-provided `limit` is passed directly to SQL. A caller could request `limit: 10000`.

**Fix:** Clamp: `let limit = req.limit.unwrap_or(5).min(50);`

---

### 14. Sequential Chunk Embedding in Ingestion

**File:** `crates/lambda-ingest/src/main.rs:100-104`

Each chunk is embedded and inserted one at a time. For 50 chunks, that's 50 serial Bedrock API calls.

**Fix:** Use `futures::stream::buffered(5)` for concurrent embedding.

---

### 15. `Dockerfile.lambda` Missing `lambda-ingest`

**File:** `Dockerfile.lambda:5`

Only builds 3 of 4 Lambdas. The CI pipeline uses `--workspace` (correct), but the Dockerfile is inconsistent.

---

## P3 — Low (Code Quality)

| Issue | File | Note |
|---|---|---|
| `insert_document` resets `created_at` on re-ingest | `store.rs:34-41` | Add `updated_at` column instead |
| Duplicated Lambda boilerplate | All 4 `main.rs` | Extract shared `OnceCell` + tracing init |
| `edition = "2024"` requires Rust 1.85+ | `Cargo.toml` | README says "Rust 1.75+" |
| Connection pool size 5 excessive for Lambda | `db.rs:9` | Single-threaded runtime needs 1-2 |
| No X-Ray distributed tracing | Lambda stack | Multi-service architecture would benefit |
| Hardcoded function names | `lambda_stack.py` | Prevents multi-environment deploys |
| No resource tagging | `app.py` | Add `cdk.Tags.of(app).add("Project", "docint")` |
| CI runs migrations after deploy | `ci.yml` | Risk of incompatible code if migration fails |
| S3 ingest permission too broad | `lambda_stack.py:23` | `docint-*/*` should reference specific bucket ARN |
| Hybrid search computes vector distance in FTS CTE | `store.rs:80-120` | Unnecessary computation on every FTS match |

---

## Testing Gaps

| Module | Status | Priority |
|---|---|---|
| `store.rs` (search, RRF, insert) | **Zero tests** | High — most critical module |
| `auth.rs` (token parsing, expiry) | **Zero tests** | Medium |
| `embeddings.rs` (serialization) | **Zero tests** | Medium |
| Lambda handlers | **Zero tests** | Medium |
| `chunker.rs` | 4 unit tests ✓ | — |
| `lambda-ingest` (key parsing) | 5 unit tests ✓ | — |
