# RAG Evaluation - User Guide

> **Status (component readiness):**
> - ✅ Infrastructure (`RagEvaluationStack`): deployed — S3 buckets + IAM role.
> - ✅ Dataset generator (`create_rag_eval_dataset.py`): rewritten and verified
>   against the real "bring your own inference" format.
> - ✅ Job creation: done via the **AWS Console** (Bedrock → Evaluations). There
>   is intentionally **no job-runner script** — the Console assembles the correct
>   `RagEvaluation` API request, which avoids hand-maintaining a fragile CLI call.

## Overview

This guide explains how to use the RAG Evaluation infrastructure to measure and optimize the quality of the document intelligence agent's hybrid search retrieval pipeline.

**Goal:** Quantify search performance and detect retrieval issues before they impact production.

## How It Works: "Bring Your Own Inference"

Our search pipeline is a custom Lambda (`docint-search`), **not** a Bedrock Knowledge Base.
For custom RAG sources, Bedrock does **not** call your retrieval pipeline. Instead:

```
┌─────────────────────────────────────────────────────────────┐
│  YOU do this (offline, ahead of time):                      │
│                                                             │
│  query → invoke docint-search → get chunks → write to JSONL │
│  (handled by scripts/create_rag_eval_dataset.py)            │
│                                                             │
├─────────────────────────────────────────────────────────────┤
│  BEDROCK does this (the eval job):                          │
│                                                             │
│  reads your JSONL → judge LLM scores each row →             │
│  ContextRelevance, ContextCoverage → results to S3          │
└─────────────────────────────────────────────────────────────┘
```

You run retrieval and embed the results in the dataset. Bedrock acts only as the
**judge** that scores those pre-computed results. This is the "bring your own
inference response data" mode of the Bedrock RAG evaluation API.

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│ RAG Evaluation Infrastructure (RagEvaluationStack)      │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  S3 Buckets:                                            │
│  ├─ docint-rag-eval-datasets-{account}                  │
│  │  └─ retrieve-only-v1.jsonl (evaluation queries)      │
│  └─ docint-rag-eval-results-{account}                   │
│     └─ {job-name}/results.jsonl (metrics)               │
│                                                         │
│  IAM Role: RagEvaluationExecutionRole                   │
│  ├─ Read datasets from S3                               │
│  ├─ Write results to S3                                 │
│  └─ Invoke Bedrock judge models (Sonnet/Haiku)          │
│                                                         │
│  NOTE: Bedrock does NOT invoke docint-search during the │
│  eval. You run retrieval beforehand (see dataset step). │
│                                                         │
│  CloudWatch Alarms: (optional, configure after baseline)│
│  └─ Alert when Context Relevance < 0.80                 │
│                                                         │
└─────────────────────────────────────────────────────────┘
         │
         ▼
┌─────────────────────────────────────────────────────────┐
│ Bedrock CreateEvaluationJob API (manual invocation)     │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  Evaluation Types:                                      │
│  ├─ retrieve-only: Test search quality independently    │
│  └─ retrieve-and-generate: Test end-to-end RAG          │
│                                                         │
│  Metrics:                                               │
│  ├─ Context Relevance (on-topic chunks?)                │
│  ├─ Context Coverage (sufficient info?)                 │
│  ├─ Correctness (factually accurate?)                   │  
│  ├─ Completeness (addresses full question?)             │
│  └─ Faithfulness (stays true to documents?)             │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

## Key Differences: RAG Evaluation vs AgentCore Evaluation

| Aspect | RAG Evaluation (this stack) | AgentCore Evaluation (evaluation_stack.py) |
|--------|----------------------------|------------------------------------------|
| **Purpose** | Measure search quality | Measure agent behavior |
| **Metrics** | Context Relevance, Context Coverage, Faithfulness | Conciseness, Tool Selection, Goal Success |
| **When to run** | On-demand, for search optimization | Always-on (10% sampling) |
| **Target** | docint-search Lambda | Full agent runtime |
| **Cost** | $1.25-2.50 per run | Included in agent runtime |
| **Integration** | Manual (AWS CLI/SDK) | Automatic (native CDK construct) |

**Use both:** AgentCore evaluations monitor ongoing agent quality, RAG evaluations optimize retrieval quality.

## Prerequisites

1. **CDK Stack Deployed**
   ```bash
   cd infrastructure
   cdk deploy DocintRagEvaluationStack
   ```

2. **AWS CLI Configured**
   - Version 2.x (required for Bedrock APIs)
   - Credentials with Bedrock and S3 permissions

3. **Evaluation Dataset Created** (see below)

## Quick Start

### 1. Create Evaluation Dataset

The dataset generator does the retrieval for you: it invokes `docint-search` for
each query and writes the results in the exact format Bedrock expects.

**Step 1a — write a starter queries file and edit it:**

```bash
python scripts/create_rag_eval_dataset.py --emit-template queries.json
```

Edit `queries.json`. Each entry needs a `query` and a `reference_answer`
(the ground-truth answer you author by hand — required for the Context coverage
metric). `tenant_id`, `limit`, and `category` are optional:

```json
[
  {
    "query": "How do I deploy a Lambda function with CDK?",
    "reference_answer": "Use the aws_lambda.Function construct and run cdk deploy.",
    "tenant_id": "default-tenant",
    "limit": 5,
    "category": "content_search"
  }
]
```

**Step 1b — build the dataset (invokes docint-search per query):**

```bash
python scripts/create_rag_eval_dataset.py \
    --queries queries.json \
    --output datasets/retrieve-only-v1.jsonl
```

> This invokes the real `docint-search` Lambda (which calls Titan for embeddings),
> so it incurs a small per-query cost. Each query that returns 0 chunks is flagged.

**Dataset format produced (JSONL — "bring your own inference"):**

Each line is a `conversationTurns` object. You author `prompt` and
`referenceResponses`; the script fills `output.retrievedResults` from the search
Lambda's response:

```json
{
  "conversationTurns": [{
    "prompt": {"content": [{"text": "How do I deploy a Lambda with CDK?"}]},
    "referenceResponses": [{"content": [{"text": "Use the construct and deploy."}]}],
    "output": {
      "knowledgeBaseIdentifier": "docint-hybrid-search",
      "retrievedResults": {
        "retrievalResults": [
          {
            "content": {"text": "<chunk content from docint-search>"},
            "metadata": {"chunk_id": "...", "document_id": "...",
                         "title": "...", "distance": "..."}
          }
        ]
      }
    }
  }]
}
```

> **Note on the old format:** earlier drafts used a flat `{query, referenceAnswer,
> expectedChunks}` shape. That is **not** what the Bedrock RAG evaluation API
> accepts — the generator now emits the correct `conversationTurns` structure above.
> `metadata` values must be strings (an AWS requirement the script enforces).

### 2. Upload Dataset to S3

```bash
# Get bucket name from stack outputs
DATASET_BUCKET=$(aws cloudformation describe-stacks \
    --stack-name DocintRagEvaluationStack \
    --query "Stacks[0].Outputs[?OutputKey=='DatasetBucketName'].OutputValue" \
    --output text)

# Upload dataset
aws s3 cp datasets/retrieve-only-v1.jsonl s3://${DATASET_BUCKET}/
```

### 3. Create the Evaluation Job (AWS Console)

There is no job-runner script. Create the job in the Console — it assembles the
correct `applicationType: RagEvaluation` request for you:

1. **Bedrock → Evaluations → Create → RAG evaluation**
2. **Evaluation type:** Retrieve only
3. **Inference source:** *Bring your own inference response* (this is what makes
   Bedrock score the chunks already in your dataset instead of calling a
   Knowledge Base)
4. **Dataset S3 URI:** `s3://<DATASET_BUCKET>/retrieve-only-v1.jsonl`
5. **Evaluator (judge) model:** Claude Sonnet (quality) or Claude Haiku (cheaper)
6. **Metrics:** Context relevance, Context coverage
7. **IAM role:** the `RagEvaluationExecutionRole` from the stack outputs
8. **Output S3 location:** `s3://<RESULTS_BUCKET>/`
9. **Create**

Get the bucket and role names from stack outputs:
```bash
aws cloudformation describe-stacks --stack-name DocintRagEvaluationStack \
    --query "Stacks[0].Outputs[*].[OutputKey,OutputValue]" --output table
```

### 4. Monitor Progress

Watch the job in the Console (Bedrock → Evaluations), or via CLI:

```bash
aws bedrock list-evaluation-jobs \
    --query 'jobSummaries[*].[jobName,status,creationTime]' \
    --output table --region us-east-1
```

Typical run time: ~5-10 min (20 prompts), ~15-20 min (50), ~30-40 min (100).

### 5. Check Results

Results are written to the results bucket and shown in the Console UI. To pull
them locally:

```bash
RESULTS_BUCKET=$(aws cloudformation describe-stacks \
    --stack-name DocintRagEvaluationStack \
    --query "Stacks[0].Outputs[?OutputKey=='ResultsBucketName'].OutputValue" \
    --output text)

aws s3 cp s3://${RESULTS_BUCKET}/<job-name>/ ./results/ --recursive
```

## Evaluation Types

### Retrieve-Only

**What it measures:** Search quality independently of generation

**Use when:**
- Optimizing chunk retrieval count (3 vs 5 vs 10)
- Testing hybrid search vs vector-only
- Validating NULL distance handling (distance=999.0 fallback)
- Comparing different embedding models

**Metrics:**
- Context Relevance: 0.80+ is good
- Context Coverage: 0.70+ is sufficient

**Cost:** ~$0.20-0.40 per 30 queries (Sonnet judge)

### Retrieve-and-Generate

**What it measures:** End-to-end RAG pipeline (search + agent response)

**Use when:**
- Validating full agent behavior
- Measuring faithfulness (hallucination detection)
- Testing conciseness improvements

**Metrics:**
- All retrieve-only metrics, plus:
- Faithfulness: 0.90+ (agent stays true to documents)
- Correctness: 0.85+ (factually accurate)
- Completeness: 0.80+ (addresses full question)

**Cost:** ~$1-2 per 30 queries (includes agent invocations)

## Interpreting Results

### Context Relevance

**Definition:** Are retrieved chunks on-topic for the query?

| Score | Interpretation | Action |
|-------|---------------|---------|
| 0.90+ | Excellent | No action needed |
| 0.80-0.89 | Good | Monitor trends |
| 0.70-0.79 | Needs improvement | Review search parameters |
| < 0.70 | Poor | Investigate hybrid search ranking |

**Common issues:**
- RRF (Reciprocal Rank Fusion) weights too aggressive
- Embedding model not capturing semantic meaning
- NULL distance fallback (999.0) polluting results

### Context Coverage

**Definition:** Do chunks contain sufficient information to answer?

| Score | Interpretation | Action |
|-------|---------------|---------|
| 0.80+ | Excellent | No action needed |
| 0.70-0.79 | Good | Consider increasing chunk retrieval count |
| 0.60-0.69 | Borderline | Increase limit or improve chunking strategy |
| < 0.60 | Insufficient | Critical - review retrieval logic |

**Common issues:**
- Retrieval limit too low (increase from 5 to 10)
- Chunks too small (review chunking strategy)
- Query not matching document structure

### Faithfulness

**Definition:** Does agent stay true to retrieved documents?

| Score | Interpretation | Action |
|-------|---------------|---------|
| 0.95+ | Excellent | No action needed |
| 0.90-0.94 | Good | Monitor for hallucination patterns |
| 0.85-0.89 | Concerning | Review agent instructions |
| < 0.85 | Critical | Agent may be fabricating data |

**Common issues:**
- Tool failure causing agent to guess
- Agent instructions too permissive
- Retrieved chunks contradictory

## Cost Management

### Budget

- **Development/tuning:** 10-20 runs/month = $12.50-50
- **Automated weekly runs:** 4 runs/month = $5-10
- **Total monthly:** $17.50-60

### Cost Optimization

**Judge model selection** (chosen in the Console "Evaluator model" step):
- **Claude Sonnet** — higher quality, ~$0.20-0.40 per 30-query run
- **Claude Haiku** — cost-effective, roughly 10x cheaper

Verify exact model IDs available in your region with
`aws bedrock list-foundation-models`.

**Dataset size:**
- Start with 20-30 queries for quick iteration
- Use 50-100 queries for production baseline
- Representative sampling > large volume

**S3 lifecycle policies:**
- Datasets: Retained indefinitely (versioned)
- Results: Transitioned to Intelligent Tiering after 30 days
- Results: Deleted after 365 days

## Troubleshooting

### "ERROR: Failed to fetch stack outputs"

**Cause:** RagEvaluationStack not deployed

**Fix:**
```bash
cd infrastructure
cdk deploy DocintRagEvaluationStack
```

### "AccessDeniedException: User is not authorized to perform: bedrock:CreateEvaluationJob"

**Cause:** IAM user lacks Bedrock permissions

**Fix:** Add to IAM user/role:
```json
{
  "Effect": "Allow",
  "Action": [
    "bedrock:CreateEvaluationJob",
    "bedrock:GetEvaluationJob",
    "bedrock:ListEvaluationJobs"
  ],
  "Resource": "*"
}
```

### "No results found" after job completion

**Cause 1:** Job still running (check status)
```bash
aws bedrock list-evaluation-jobs --status-equals InProgress
```

**Cause 2:** Job failed (check CloudWatch Logs)
```bash
aws logs tail /aws/bedrock/model-evaluation-jobs --follow
```

**Cause 3:** S3 permissions issue (check evaluation role)

### Low Context Relevance scores

**Investigate:**
1. Review sample queries vs retrieved chunks manually
2. Test vector-only vs hybrid search
3. Check NULL distance handling (distance=999.0)
4. Validate embedding model is appropriate

**Quick test:**
```bash
# Invoke docint-search Lambda directly
aws lambda invoke \
    --function-name docint-search \
    --payload '{"query": "test query", "limit": 5}' \
    response.json

cat response.json | jq '.chunks[] | {score: .score, text: .text[0:100]}'
```

### Evaluation job stuck "InProgress"

**Cause:** Large dataset or throttling

**Wait time:**
- 20 queries: ~5-10 minutes
- 50 queries: ~15-20 minutes
- 100 queries: ~30-40 minutes

**Check progress:**
```bash
aws bedrock get-evaluation-job \
    --job-identifier arn:aws:bedrock:us-east-1:123456789012:evaluation-job/xxxxx \
    --query '[status,creationTime]' \
    --output table
```

## Automation (Future Enhancement)

For now, evaluation jobs are created manually in the Console — appropriate for
periodic, on-demand search-quality checks. If you later want recurring runs,
the path would be a scheduled Lambda (EventBridge cron) that calls
`bedrock:CreateEvaluationJob` with the same `RagEvaluation` config the Console
produces. Capture a known-good request first by inspecting a Console-created job
(or `aws bedrock get-evaluation-job`) so the automated call matches the verified
API shape, rather than hand-writing it.

A pre-deployment quality gate (fail a release if Context Relevance drops below a
threshold) is a natural extension of that scheduled job, but is out of scope
until a baseline is established.

## Related Documentation

- [TODO-RAG-EVALUATION.md](TODO-RAG-EVALUATION.md): Complete implementation plan
- [PRIORITY-ISSUES.md](PRIORITY-ISSUES.md): Current system priorities
- [evaluation_stack.py](../infrastructure/stacks/evaluation_stack.py): AgentCore evaluation implementation

## FAQ

### Q: Do I need both RAG evaluation and AgentCore evaluation?

**A:** Yes, they serve different purposes:
- **AgentCore evaluation** (always-on): Monitors agent behavior, tool usage, goal success
- **RAG evaluation** (on-demand): Optimizes search quality, measures retrieval performance

### Q: How often should I run RAG evaluations?

**A:**
- **Development:** After each search optimization change
- **Production:** Weekly baseline + before major releases
- **Incident response:** When users report irrelevant search results

### Q: Can I use this with Bedrock Knowledge Bases instead of custom Lambda?

**A:** Yes. In the Console, choose a **Bedrock Knowledge Base** as the inference
source instead of "Bring your own inference response." Bedrock then performs the
retrieval itself and you skip the dataset-generation step (no need to run
`create_rag_eval_dataset.py`) — your dataset only needs `prompt` and
`referenceResponses`. Our system uses a custom Lambda, so we use the
bring-your-own-inference path documented above.

### Q: What's the difference between this and unit tests?

**A:**
- **Unit tests:** Functional correctness (does search return results?)
- **RAG evaluation:** Quality assessment (are results relevant and sufficient?)

Both are needed for comprehensive testing.

### Q: Can I use a different judge model?

**A:** Yes — pick the evaluator model in the Console when creating the job.
Claude Sonnet (best quality) and Claude Haiku (cost-effective) are both common
choices. Confirm exact model IDs available in your region with
`aws bedrock list-foundation-models`.

## Support

For issues or questions:
1. Check this guide's Troubleshooting section
2. Review [TODO-RAG-EVALUATION.md](TODO-RAG-EVALUATION.md) for implementation details
3. Check AWS Bedrock documentation: https://docs.aws.amazon.com/bedrock/latest/userguide/knowledge-base-evaluation.html
4. Inspect CloudWatch Logs: `/aws/bedrock/model-evaluation-jobs`
