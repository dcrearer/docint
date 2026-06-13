# Fix: TenantInjectorMCPClient Tool Discovery Issue

**Date:** 2026-06-13  
**Status:** ✅ FIXED - Ready for deployment  
**Severity:** P0 - Tools completely broken since June 7

---

## Problem Summary

### Symptoms
- Agent generates tool call XML but tools never execute
- Zero Lambda invocations for 6 days (last call: June 7)
- Agent hallucinates responses with fake data
- Poor evaluation scores: 2% GoalSuccessRate, 33% Correctness
- Logs show "TenantInjectorMCPClient initialized" but NEVER "Tool call:"

### Root Cause
The `TenantInjectorMCPClient` wrapper class breaks Strands Agent's tool discovery mechanism:

1. **Strands Agent** looks for a `tools` attribute during initialization to discover available tools
2. **TenantInjectorMCPClient** wraps the `MCPClient` but doesn't expose the `tools` property
3. **Result:** Strands can't discover tools → generates XML but never executes → zero Lambda calls

**File:** `agent/agent.py`, lines 93-109 (original broken implementation)

---

## Why We Need The Wrapper

**Cannot remove the wrapper** because all Lambda functions require `tenant_id`:

```rust
// crates/lambda-search/src/main.rs:11-14
#[derive(Deserialize)]
struct Request {
    query: String,
    tenant_id: String,  // ← REQUIRED
    limit: Option<i64>,
}
```

The MCP Gateway (AWS Bedrock MCP Gateway) only provides:
- IAM authentication (SigV4)
- Request routing
- **NO custom parameter injection**

Therefore, the agent code is the ONLY place we can inject `tenant_id` before calling Lambda.

---

## The Fix

### What Was Missing
The wrapper needed to explicitly expose the `tools` property that Strands looks for during Agent initialization.

### Implementation

Added three methods to `TenantInjectorMCPClient`:

1. **`__dir__()`** - Expose all attributes for introspection
2. **`@property tools`** - **CRITICAL** - Proxy the tools list from underlying MCP client
3. Updated docstring to explain MCP protocol proxying

### Changes

```python
class TenantInjectorMCPClient:
    """Wrapper that injects tenant_id into all MCP tool calls.

    Properly proxies MCP protocol to work with Strands Agent tool discovery.
    """

    # ... existing __init__ and __call__ methods ...

    def __dir__(self):
        """Expose all attributes from wrapped client for introspection."""
        return dir(self.mcp_client)

    @property
    def tools(self):
        """Proxy tools property for Strands Agent tool discovery.

        This is critical for Strands to discover available tools during
        Agent initialization. Without this explicit property, Strands
        cannot see the tools from the wrapped MCP client.
        """
        if hasattr(self.mcp_client, 'tools'):
            tools = self.mcp_client.tools
            logger.info(f"Exposing {len(tools) if isinstance(tools, (list, dict)) else 'unknown'} tools from MCP client")
            return tools
        logger.warning("MCP client has no 'tools' attribute")
        return []
```

---

## Testing

### Local Test Results ✅

Created comprehensive test suite: `agent/test_tenant_injector.py`

**All 5 tests passed:**

1. ✅ **Tool Discovery** - `wrapper.tools` returns 3 tools
2. ✅ **Attribute Delegation** - `__getattr__` fallthrough works
3. ✅ **Tool Call Interception** - `tenant_id` injected correctly
4. ✅ **Lambda Validation** - Lambda receives `tenant_id`, rejects calls without it
5. ✅ **Introspection** - `dir(wrapper)` includes 'tools'

**Test output:**
```
============================================================
✅ ALL TESTS PASSED - Wrapper is ready for deployment!
============================================================
```

### What The Fix Enables

**Before fix:**
```
Agent initialization → Strands looks for tools
  → wrapper.tools NOT FOUND
  → Strands thinks: "no tools available"
  → Agent generates XML but never executes
  → Zero Lambda calls
```

**After fix:**
```
Agent initialization → Strands looks for tools
  → wrapper.tools FOUND (3 tools exposed)
  → Strands registers: search-documents, get-document-metadata, compare-documents
  → Agent generates XML AND executes via wrapper.__call__()
  → tenant_id injected automatically
  → Lambda functions receive requests with tenant_id ✅
```

---

## Deployment Plan

### 1. Pre-Deployment Checklist
- [x] Local tests pass
- [x] Changes reviewed and documented
- [ ] Commit changes to git
- [ ] Push to trigger CI/CD

### 2. Deployment Steps

```bash
# Commit the fix
git add agent/agent.py agent/test_tenant_injector.py docs/FIX-TENANT-INJECTOR.md
git commit -m "fix(agent): expose tools property in TenantInjectorMCPClient for Strands discovery

Root cause: Wrapper broke tool discovery by not exposing the 'tools'
attribute that Strands Agent looks for during initialization.

Result: Tools haven't worked since June 7 - zero Lambda invocations,
agent hallucinates responses (2% GoalSuccessRate, 33% Correctness).

Fix: Add @property tools to proxy MCP client's tools list.
Also add __dir__() for proper introspection.

Tests: All 5 local tests pass (see test_tenant_injector.py)

Co-Authored-By: Claude Sonnet 4.5 <noreply@anthropic.com>"

# Push to trigger deployment
git push origin main
```

### 3. Verification Steps (After Deployment)

**Step 1: Check agent logs for tool discovery**
```bash
aws logs tail /aws/bedrock-agentcore/runtimes/docint_agent-lsc56PDJsX-docint_agent_endpoint \
  --follow --since 5m | grep -E "(Exposing.*tools|Tool call:)"
```

**Expected:**
```
Exposing 3 tools from MCP client
Tool call: search-documents with injected tenant_id=<tenant>
```

**Step 2: Manual test with CLI**
```bash
cargo run --bin docint-cli
# Type: list my documents
# Type: search for observability
# Type: quit
```

**Step 3: Verify Lambda invocations**
```bash
aws cloudwatch get-metric-statistics \
  --namespace AWS/Lambda \
  --metric-name Invocations \
  --dimensions Name=FunctionName,Value=docint-search \
  --start-time $(date -u -d '5 minutes ago' +%Y-%m-%dT%H:%M:%S) \
  --end-time $(date -u +%Y-%m-%dT%H:%M:%S) \
  --period 60 \
  --statistics Sum
```

**Expected:** Non-zero invocations

**Step 4: Check Lambda logs**
```bash
aws logs tail /aws/lambda/docint-search --follow
```

**Expected:** New log entries with tenant_id in requests

---

## Rollback Plan

If the fix causes issues:

1. **Immediate rollback via git:**
   ```bash
   git revert HEAD
   git push origin main
   ```

2. **Wait for CI/CD to redeploy previous version (~10-15 minutes)**

3. **Alternative: Manual hotfix**
   - Remove wrapper entirely (temporary)
   - Modify Lambda functions to use a default tenant_id
   - Redeploy

---

## Success Metrics

### Immediate (5 minutes post-deployment)
- ✅ Agent logs show "Exposing 3 tools from MCP client"
- ✅ Agent logs show "Tool call: {name} with injected tenant_id="
- ✅ Lambda functions receive invocations

### Short-term (1 hour post-deployment)
- ✅ CLI queries return real document data (not hallucinations)
- ✅ Lambda invocation count > 0 in CloudWatch
- ✅ No Lambda errors related to missing tenant_id

### Long-term (24 hours post-deployment)
- ✅ Evaluation scores improve:
  - GoalSuccessRate: 2% → >80%
  - Correctness: 33% → >70%
- ✅ Dashboard shows Lambda metrics (was empty before)

---

## Related Issues

- **Tools broken since:** June 7, 23:35 UTC
- **Downtime:** 6 days (June 7-13)
- **Affected evaluations:** 365 evaluations from load test (all with hallucinated data)
- **Related commits:**
  - e240a3b - Environment property fix
  - 2533130 - Priority issues update
  - fdf16b6 - Migration fix

---

## Lessons Learned

1. **Wrapper patterns are tricky** - Always test that wrapper classes properly expose ALL attributes needed by the framework
2. **Silent failures are dangerous** - Agent appeared to work (generated XML) but tools never executed
3. **Log strategically** - "Tool call:" log never appeared was the key diagnostic clue
4. **Test locally first** - Created comprehensive test suite before deploying to production
5. **Understand the full stack** - Had to understand: Agent → Strands → MCP → Gateway → Lambda to diagnose the issue

---

## Technical References

- **Strands Agent tool discovery:** Looks for `tools` attribute during `Agent(tools=[...])` initialization
- **MCP Protocol:** Standard protocol for model-context communication (tool definitions, execution)
- **Python property delegation:** Must use `@property` decorator, not just `__getattr__`, for class attributes
- **AgentCore Runtime:** Managed service that hosts the agent with built-in OTEL instrumentation

---

## Next Steps

1. ✅ Fix implemented and tested locally
2. ⏳ Commit and push to trigger deployment
3. ⏳ Monitor deployment logs
4. ⏳ Run manual verification tests
5. ⏳ Re-run load test to generate new evaluations with real tool data
6. ⏳ Compare evaluation scores before/after fix
7. ⏳ Update monitoring dashboard if needed
