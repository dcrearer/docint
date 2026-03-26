# Document Intelligence Service

A production-grade RAG (Retrieval-Augmented Generation) system built with:
- **Rust Lambda functions** for vector search (5-10x faster than Python)
- **AgentCore Runtime** hosting a Claude-powered agent
- **AgentCore Gateway** exposing Lambda functions as MCP tools
- **PostgreSQL + pgvector** for hybrid vector + full-text search

## Architecture

```
Client → AgentCore Runtime (Claude) → AgentCore Gateway (MCP)
                                            │
                              ┌─────────────┼─────────────┐
                              ▼             ▼             ▼
                        lambda-search  lambda-metadata  lambda-compare
                              │             │             │
                              └─────────────┼─────────────┘
                                            ▼
                                   Aurora PostgreSQL
                                     + pgvector
```

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
│   ├── lambda-ingest/          # S3 ingestion pipeline Lambda
│   └── docint-cli/             # Local dev/test CLI
├── agent/
│   ├── agent.py                # Strands agent (Claude + Gateway tools)
│   └── requirements.txt
├── infrastructure/             # CDK (Python)
│   ├── app.py                  # 5 stacks wired together
│   └── stacks/
│       ├── database_stack.py   # Aurora Serverless v2 + VPC endpoints
│       ├── lambda_stack.py     # 4 Lambdas (VPC, X-Ray, IAM)
│       ├── gateway_stack.py    # AgentCore Gateway + MCP tool targets
│       ├── agent_stack.py      # AgentCore Runtime + Endpoint
│       └── monitoring_stack.py # CloudWatch dashboard + alarms
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

# 4. Run the CLI (seeds data + searches)
cargo run --bin docint-cli

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
2. **Build** — `cargo lambda build --release --arm64`
3. **Deploy** — `cdk deploy --all`

### Manual deploy

```bash
# Build Lambdas (on Linux, or use Dockerfile.lambda on macOS)
cargo lambda build --release --arm64 --workspace

# Deploy all stacks
cd infrastructure
source .venv/bin/activate
pip install -r requirements.txt
cdk deploy --all
```

## Key Design Decisions

- **Hybrid search with RRF** — combines vector similarity and PostgreSQL full-text search for better recall than either alone
- **Row-level security** — PostgreSQL enforces tenant isolation at the DB level, not just application code
- **VPC endpoints instead of NAT** — saves ~$32/month while keeping Lambdas in isolated subnets
- **OnceCell for Lambda state** — DB pool and embedder initialize once on cold start, reused across invocations
- **Standalone Embedder** — decoupled from VectorStore so ingestion and search can share it independently

## Cost Estimate (Demo)

| Component | Monthly |
|---|---|
| Aurora Serverless v2 (min capacity) | ~$15 |
| Lambda (1K invocations) | ~$0.50 |
| AgentCore Runtime (1K) | ~$2 |
| Bedrock Claude (1K conversations) | ~$3 |
| Bedrock Embeddings | ~$0.30 |
| VPC Endpoints | ~$21 |
| **Total** | **~$42** |
