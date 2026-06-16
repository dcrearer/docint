# TODO: Implement Bedrock RAG Evaluation

**Created:** 2026-06-16  
**Priority:** P3 (Low - Optimization)  
**Status:** 📋 Not Started

---

## Overview

Implement AWS Bedrock RAG Evaluation to measure and optimize the quality of our hybrid search retrieval pipeline. This complements existing AgentCore evaluations by providing RAG-specific metrics on retrieval quality, faithfulness, and context relevance.

**Goal:** Quantify search performance and detect retrieval issues before they impact production.

---

## Current State

### What We Have
- ✅ AgentCore Evaluations (agent behavior, tool usage, response quality)
- ✅ Hybrid search (vector + full-text with RRF ranking)
- ✅ PostgreSQL with pgvector embeddings
- ✅ 4 Lambda tools: metadata, ingest, search, compare

### What We're Missing
- ❌ **Retrieval quality metrics** - Are we retrieving the right chunks?
- ❌ **Faithfulness measurement** - Is the agent staying true to retrieved documents?
- ❌ **Search optimization data** - No quantitative basis for tuning hybrid search parameters

### Evidence from Current Evaluations

**From AgentCore evaluation report (2026-06-13):**
```
Conciseness:    5.8%  🔴 CRITICAL - Extremely verbose responses
Correctness:   86.3%  🟢 Good factual accuracy
Goal Success:  92.5%  🟢 Tasks completing successfully
```

**Key questions RAG Evaluation would answer:**
1. Is verbose output caused by retrieving too many/too detailed chunks?
2. Are we retrieving irrelevant context that confuses the agent?
3. Is the 86.3% correctness limited by retrieval quality?
4. How often does the agent fabricate data when retrieval fails?

---

## Why Bedrock RAG Evaluation?

### Strong Fit for Our Architecture

| Our Setup | RAG Eval Capability | Benefit |
|-----------|---------------------|---------|
| Hybrid search (vector + full-text) | Context Relevance metric | Measure if search strategy is optimal |
| Custom chunk retrieval (not KB) | "Bring Your Own Inference" support | Can evaluate our Lambda-based search |
| Agent using Claude Haiku | Faithfulness metric | Detect hallucination beyond retrieved docs |
| Recent NULL distance bug | Context Coverage metric | Verify fallback handling doesn't hurt quality |

### Key Metrics We Need

| Metric | Measures | Priority | Use Case |
|--------|----------|----------|----------|
| **Context Relevance** | Are retrieved chunks on-topic? | 🔴 Critical | Validate hybrid search configuration |
| **Context Coverage** | Do chunks contain sufficient info? | 🔴 Critical | Optimize retrieval count (3 vs 5 vs 10) |
| **Faithfulness** | Agent stays true to documents? | 🟡 High | Detect fabrication when tools fail |
| **Correctness** | Final answer accuracy | 🟡 High | Already have via AgentCore, but RAG view useful |
| **Completeness** | Answer addresses all parts? | 🟢 Medium | Important for multi-part queries |
| **Citation Precision** | Citations accurate? | ⚪ Low | Not using citations currently |

---

## Implementation Plan

### Phase 1: Dataset Creation (Effort: 2-4 hours)

**Goal:** Create evaluation dataset with ground truth

**Tasks:**
1. Extract 20-30 representative queries from production logs
2. For each query, manually identify:
   - Expected chunks that should be retrieved (ground truth)
   - Expected final answer (reference answer)
3. Store in S3 as JSONL format per Bedrock requirements

**Dataset structure:**
```json
{
  "query": "What documents do I have about AWS Lambda?",
  "expected_chunks": [
    {"document_id": "...", "chunk_id": "...", "content": "..."}
  ],
  "reference_answer": "You have 3 documents about Lambda: ...",
  "category": "metadata_query"
}
```

**Categories to cover:**
- Metadata queries (list documents, get titles)
- Content search (semantic vector search)
- Comparison queries (compare multiple documents)
- Edge cases (empty results, no embeddings)

---

### Phase 2: Retrieve-Only Evaluation (Effort: 4-6 hours)

**Goal:** Test search quality independently

**Setup:**
1. Create IAM role for Bedrock Evaluation execution
2. Configure S3 buckets for dataset and results
3. Create evaluation job via AWS Console or API

**Configuration:**
```python
evaluation_job = {
    "jobName": "docint-search-quality-v1",
    "evaluationType": "RETRIEVE_ONLY",
    "evaluatorModelId": "anthropic.claude-sonnet-4-20250514-v1:0",
    "inferenceConfig": {
        "customInference": {
            "dataSourceArn": "arn:aws:lambda:us-east-1:404235888080:function:docint-search"
        }
    },
    "metrics": [
        "CONTEXT_RELEVANCE",
        "CONTEXT_COVERAGE"
    ],
    "datasetLocation": "s3://docint-evaluations/datasets/retrieve-only-v1.jsonl"
}
```

**Success criteria:**
- Context Relevance > 0.80 = Good retrieval
- Context Coverage > 0.70 = Sufficient information

**Experiments to run:**
- Baseline: Current hybrid search (limit=5)
- Variant A: Vector-only search (limit=5)
- Variant B: Hybrid search (limit=3)
- Variant C: Hybrid search (limit=10)

---

### Phase 3: Retrieve-and-Generate Evaluation (Effort: 3-4 hours)

**Goal:** Test end-to-end RAG pipeline

**Setup:**
- Use same dataset from Phase 1
- Add full agent invocation (not just search Lambda)
- Measure generation quality in addition to retrieval

**Configuration:**
```python
evaluation_job = {
    "jobName": "docint-rag-end-to-end-v1",
    "evaluationType": "RETRIEVE_AND_GENERATE",
    "evaluatorModelId": "anthropic.claude-sonnet-4-20250514-v1:0",
    "metrics": [
        "CONTEXT_RELEVANCE",
        "CONTEXT_COVERAGE",
        "CORRECTNESS",
        "COMPLETENESS",
        "FAITHFULNESS"  # Key metric for hallucination detection
    ]
}
```

**Success criteria:**
- Faithfulness > 0.90 = Agent stays true to documents
- Completeness > 0.80 = Addresses full question
- Correctness > 0.85 = Matches/exceeds current 86.3%

---

### Phase 4: Integration & Automation (Effort: 4-6 hours)

**Goal:** Make RAG evaluation repeatable and actionable

**Tasks:**
1. Create CDK stack for evaluation infrastructure
   - IAM roles
   - S3 buckets for datasets/results
   - CloudWatch alarms for metric thresholds
2. Add evaluation scripts to `/scripts/`:
   - `scripts/run_rag_eval.sh` - Launch evaluation job
   - `scripts/check_rag_results.sh` - Parse results from S3
3. Document in `docs/RAG-EVALUATION.md`:
   - How to create datasets
   - How to run evaluations
   - How to interpret metrics
   - Baseline scores for comparison

**Automation options (future):**
- Weekly scheduled evaluation on production traffic sample
- Pre-deployment evaluation gate (like current CI/CD tests)
- Alert when metrics drop below thresholds

---

## Cost Estimate

**Per evaluation run (30 queries):**

| Component | Cost |
|-----------|------|
| Judge model calls (Claude Sonnet 4) | ~$0.20-0.40 |
| Search Lambda invocations | ~$0.01 |
| Agent invocations (retrieve-and-generate) | ~$1-2 |
| S3 storage (datasets + results) | ~$0.01 |
| **Total per run** | **~$1.25-2.50** |

**Monthly estimate:**
- Development/tuning: 10-20 runs = $12.50-50
- Automated weekly runs: 4 runs = $5-10
- **Total monthly:** $17.50-60

**ROI:**
- Manual QA testing: 4-8 hours/month saved
- Prevent production retrieval issues: High value
- Data-driven search optimization: High value

---

## Success Metrics

### Immediate (After Phase 1-2)
- ✅ Baseline Context Relevance score established
- ✅ Optimal chunk retrieval count identified (3/5/10/20)
- ✅ Hybrid vs vector-only search comparison

### Short-term (After Phase 3)
- ✅ Faithfulness baseline > 0.90
- ✅ Detect fabrication patterns when tools fail
- ✅ Quantify impact of NULL distance fallback (999.0)

### Long-term (After Phase 4)
- ✅ Automated weekly evaluations running
- ✅ Alerting on metric degradation
- ✅ Pre-deployment evaluation gate
- ✅ Historical trend data for search quality

---

## Open Questions

1. **Dataset size:** Start with 30 or go straight to 100?
   - **Recommendation:** Start with 30, expand based on findings

2. **Judge model:** Sonnet 4.0 vs Haiku 4.5?
   - **Recommendation:** Sonnet 4.0 for higher quality evaluation
   - Alternative: Haiku 4.5 for cost savings (10x cheaper)

3. **Integration with AgentCore evals:** Keep separate or merge?
   - **Recommendation:** Keep separate - different purposes
   - AgentCore = Always on (10% sampling), agent behavior
   - RAG Eval = On-demand, search optimization

4. **Custom metrics:** Need brand voice or rubric evaluation?
   - **Recommendation:** Start with built-in metrics
   - Add custom metrics if needed after baseline

---

## Related Work

**AgentCore Evaluation Stack:**
- `infrastructure/stacks/evaluation_stack.py` - CDK-managed evaluation config
- 6 built-in evaluators (Conciseness, Correctness, GoalSuccessRate, etc.)
- 10% sampling in production, 100% in dev

**Search Pipeline:**
- `crates/lambda-search/src/main.rs` - Hybrid search Lambda
- `crates/docint-core/src/store.rs` - Vector store with RRF ranking
- Recent fix: NULL distance handling (unwrap_or(999.0))

**Existing Issues:**
- Conciseness: 5.8% (agent too verbose)
- Possible root cause: Retrieved chunks too detailed or too many?
- RAG Evaluation would confirm/reject this hypothesis

---

## References

**AWS Documentation:**
- [Bedrock RAG Evaluation Overview](https://docs.aws.amazon.com/bedrock/latest/userguide/evaluation-kb.html)
- [Creating RAG Evaluation Jobs](https://docs.aws.amazon.com/bedrock/latest/userguide/knowledge-base-evaluation-create-randg.html)
- [Supported Models and Metrics](https://docs.aws.amazon.com/bedrock/latest/userguide/evaluation-support.md)

**Internal:**
- AgentCore Evaluation Report: 2026-06-13 (934 evaluations, 141 sessions)
- NULL Distance Fix: commit e240a3b (2026-06-14)
- Priority Issues: `docs/PRIORITY-ISSUES.md` (all P0-P3 complete)

---

## Next Steps

1. **Immediate:** Review and approve this TODO
2. **Week 1-2:** Create evaluation dataset (Phase 1)
3. **Week 3:** Run retrieve-only evaluations (Phase 2)
4. **Week 4:** Run retrieve-and-generate evaluations (Phase 3)
5. **Week 5-6:** Automation and documentation (Phase 4)

**Total timeline:** 5-6 weeks, ~18-24 hours effort

**Blocker:** None - can start immediately after approval

---

## Notes

- RAG Evaluation is **complementary** to AgentCore Evaluations, not a replacement
- Focus on retrieve-only first (cheaper, faster iteration)
- Bring-Your-Own-Inference support means we can evaluate our custom Lambda-based search
- Success will be measured by improved search quality metrics, not just scores
