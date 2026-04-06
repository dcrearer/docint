# Document Intelligence Service

A production-grade RAG (Retrieval-Augmented Generation) system built with:
- **Rust Lambda functions** for vector search (5-10x faster than Python)
- **AgentCore Runtime** hosting a Claude-powered agent (container deployment, ARM64)
- **AgentCore Gateway** exposing Lambda functions as MCP tools (SigV4 auth)
- **PostgreSQL + pgvector** for hybrid vector + full-text search
- **S3 auto-ingest** — drop a file, it's automatically chunked, embedded, and searchable
- **Cognito authentication** — CLI sign-up/login with tenant isolation via Cognito `sub` (UUID)

![Architecture](docs/architecture.png)

## Project Structure

```
docint/
├── crates/                     # Rust workspace
│   ├── docint-core/            # Shared library
│   │   ├── src/
│   │   │   ├── chunker.rs      # Semantic text chunking
│   │   │   ├── db.rs           # Connection pool + RLS tenant context
│   │   │   ├── embeddings.rs   # Bedrock Titan v2 embeddings
│   │   │   ├── models.rs       # Data types (Document, Chunk, SearchResult)
│   │   │   └── store.rs        # Vector store (similarity, hybrid, RRF)
│   │   └── Cargo.toml
│   ├── lambda-search/          # Hybrid search Lambda
│   ├── lambda-metadata/        # Document metadata Lambda
│   ├── lambda-compare/         # Document comparison Lambda
│   ├── lambda-ingest/          # S3 ingestion pipeline Lambda (S3 events + direct invoke)
│   └── docint-cli/             # Rust CLI client for querying the agent
│       └── src/
│           ├── main.rs         # CLI entry point, chat REPL
│           └── auth.rs         # Cognito auth (login, signup, token cache)
├── agent/
│   ├── agent.py                # Strands agent (Claude Haiku + Gateway tools via mcp-proxy-for-aws)
│   ├── Dockerfile              # ARM64 container for AgentCore Runtime
│   └── requirements.txt
├── infrastructure/             # CDK (Python)
│   ├── app.py                  # 6 stacks wired together
│   └── stacks/
│       ├── auth_stack.py       # Cognito User Pool + auto-confirm trigger
│       ├── database_stack.py   # Aurora Serverless v2 + VPC endpoints
│       ├── lambda_stack.py     # 4 Lambdas (VPC, X-Ray, IAM) + S3 bucket with event triggers
│       ├── gateway_stack.py    # AgentCore Gateway + MCP tool targets
│       ├── agent_stack.py      # AgentCore Runtime (container) + Endpoint + Memory
│       └── monitoring_stack.py # CloudWatch dashboard + alarms
├── docs/ 
│   └── architecture.png        # Architecture diagram
├── migrations/                 # SQL migrations (sqlx)
├── local/                      # Podman compose + test events
├── .github/workflows/ci.yml   # CI/CD pipeline
├── Cargo.toml                  # Workspace config + release profile
└── Dockerfile.lambda           # Container-based cross-compile
```

## Prerequisites

- Rust 1.75+
- Python 3.11+
- Podman & Podman Compose
- AWS CLI v2 (configured with Bedrock access)
- cargo-lambda (`cargo install cargo-lambda`)
- AWS CDK (`npm install -g aws-cdk`)

## Quick Start — Query the Agent

```bash
# Set environment variables (from CDK outputs)
export DOCINT_RUNTIME_ARN="arn:aws:bedrock-agentcore:us-east-1:<ACCOUNT_ID>:runtime/<RUNTIME_ID>"
export DOCINT_CLIENT_ID="<COGNITO_CLIENT_ID>"

# Launch interactive chat (sign up or login on first run)
cargo run --bin docint-cli -- --chat
```

On first run you'll see:
```
Welcome to docint

> Login
  Sign up
  Quit
```

Sign up creates an account and logs you in immediately. Your tenant ID is your Cognito `sub` (UUID) — unique, collision-free, and used for all data isolation.

Chat commands:
- Type a question to query the agent
- `logout` — clear cached credentials and exit
- `quit` / `exit` — exit

## Ingest Documents

Upload files to the S3 bucket — they're automatically ingested. Tenant is derived from the key prefix:

```bash
# Ingest under tenant-1
aws s3 cp my-notes.md s3://docint-docs-<ACCOUNT_ID>/tenant-1/notes/

# Ingest under tenant-2
aws s3 cp report.md s3://docint-docs-<ACCOUNT_ID>/tenant-2/docs/

# Files without a tenant prefix fall back to DEFAULT_TENANT_ID (default: tenant-1)
aws s3 cp misc.txt s3://docint-docs-<ACCOUNT_ID>/misc/
```

Supported formats: `.txt` `.md` `.csv` `.json` `.html` `.xml` `.yaml` `.yml` `.log` `.rst`

Or invoke the ingest Lambda directly:

```bash
aws lambda invoke --function-name docint-ingest \
  --cli-binary-format raw-in-base64-out \
  --payload '{"bucket":"docint-docs-<ACCOUNT_ID>","key":"docs/guide.txt","tenant_id":"tenant-1","title":"My Guide"}' \
  /tmp/out.json
```

## Local Development

```bash
# 1. Start PostgreSQL
cd local && podman-compose up -d

# 2. Run migrations
export DATABASE_URL="postgres://docint:docint_local@localhost:5432/docint"
sqlx migrate run --source migrations

# 3. Build and test
cargo build --workspace
cargo test --lib --workspace

# 4. Test the agent locally (requires AWS credentials)
cd agent && python3 -m venv .venv && source .venv/bin/activate
pip install -r requirements.txt
export GATEWAY_URL="https://<GATEWAY_ID>.gateway.bedrock-agentcore.us-east-1.amazonaws.com/mcp"
export MODEL_ID="us.anthropic.claude-haiku-4-5-20251001-v1:0"
python3 -c "from agent import invoke; print(invoke({'prompt':'What documents do you have?','tenant_id':'tenant-1'}))"

# 5. Test a Lambda locally
cargo lambda watch --invoke-port 9001 &
cargo lambda invoke lambda-search \
  --data-file local/test-events/search.json \
  --invoke-port 9001
```

## Deployment

### First-time setup

```bash
# 1. Bootstrap CDK (if not done)
cd infrastructure
cdk bootstrap

# 2. Deploy GitHub OIDC role (update OWNER/docint in the file first)
cdk deploy -a "python3 bootstrap_github_oidc.py"

# 3. Add the output role ARN as GitHub secret: AWS_DEPLOY_ROLE_ARN
```

### Deploy via CI/CD

Push to `main` triggers the GitHub Actions pipeline:
1. **Test** — `cargo test --lib --workspace`
2. **Build** — `cargo lambda build --release --arm64` + Docker image (QEMU cross-compile)
3. **Deploy** — `cdk deploy --all`

### Manual deploy

```bash
cargo lambda build --release --arm64 --workspace

cd infrastructure
source .venv/bin/activate
pip install -r requirements.txt
cdk deploy --all
```

## Key Design Decisions

- **Hybrid search with RRF** — combines vector similarity and PostgreSQL full-text search for better recall than either alone
- **RRF stands for Reciprocal Rank Fusion** — combines two different search strategies into one ranked result set.
- **Row-level security** — PostgreSQL enforces tenant isolation at the DB level, not just application code
- **VPC endpoints instead of NAT** — saves ~$32/month while keeping Lambdas in isolated subnets
- **OnceCell for Lambda state** — DB pool and embedder initialize once on cold start, reused across invocations
- **Standalone Embedder** — decoupled from VectorStore so ingestion and search can share it independently
- **Container-based agent** — ARM64 Docker image for AgentCore Runtime avoids cold start timeouts
- **SigV4 Gateway auth** — `mcp-proxy-for-aws` handles IAM signing for MCP connections
- **Claude Haiku** — faster responses (~5-7s) vs Sonnet (~15s) for interactive use
- **S3 event-driven ingestion** — upload a file, it's automatically chunked, embedded, and stored
- **Tenant-from-key derivation** — S3 key prefix (e.g., `tenant-2/docs/file.md`) determines tenant_id automatically
- **Conversational memory** — AgentCore Memory with semantic, summary, and user preference strategies for cross-session recall
- **Cognito `sub` as tenant ID** — UUID assigned by Cognito on sign-up, guarantees no collision, used for RLS and data isolation
- **Auto-confirm Lambda trigger** — skips email verification for fast sign-up; easily reverted to email verification for production

## TODO

### Conversational Memory (AgentCore Memory)

- [x] Infrastructure: add AgentCore Memory resource to `agent_stack.py` (semantic strategy, 30-day event expiry)
- [x] Infrastructure: pass `MEMORY_ID` env var to agent container + IAM permissions for memory data plane
- [x] Agent: integrate `AgentCoreMemorySessionManager` with Strands agent
- [x] Agent: handle empty memory (first conversation) and write failures gracefully
- [x] CLI: add `--chat` flag for interactive REPL mode with streaming responses
- [x] CLI: add `--actor` flag, generate `session_id` per session, include both in payload
- [x] Docs: update README with `--chat` usage, `MEMORY_ID` env var
- [ ] Test: verify cross-session memory retrieval, tenant isolation, failure resilience

### Ingestion

- [x] S3 auto-ingest: derive `tenant_id` from S3 key prefix (e.g., `tenant-2/docs/file.md` → `tenant-2`)

### Performance

- [ ] Tighter system prompt to reduce output tokens
- [ ] Single-turn tool use (skip tool selection LLM round-trip)
- [ ] Reduce search results returned to minimize synthesis input
- [ ] Evaluate Amazon Nova Micro/Lite as faster model alternatives
- [ ] Lambda-based agent (eliminate AgentCore Runtime + MCP Gateway overhead)
- [ ] Query response cache (DynamoDB/ElastiCache)

## Cost Estimate (Demo)

| Component | Monthly |
|---|---|
| Aurora Serverless v2 (min capacity) | ~$15 |
| Lambda (1K invocations) | ~$0.50 |
| AgentCore Runtime (1K) | ~$2 |
| Bedrock Claude Haiku (1K conversations) | ~$1 |
| Bedrock Embeddings | ~$0.30 |
| VPC Endpoints | ~$21 |
| S3 (document storage) | ~$0.02 |
| **Total** | **~$40** |
