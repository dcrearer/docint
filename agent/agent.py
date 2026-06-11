"""Document Intelligence Agent — runs on AgentCore Runtime."""
import os
import sys
import logging
import traceback
from typing import Any, Dict

logging.basicConfig(level=logging.INFO, stream=sys.stderr)
logger = logging.getLogger(__name__)

from bedrock_agentcore import BedrockAgentCoreApp
from strands import Agent
from strands.models import BedrockModel
from strands.tools.mcp import MCPClient
from mcp_proxy_for_aws.client import aws_iam_streamablehttp_client
from bedrock_agentcore.memory.integrations.strands.config import AgentCoreMemoryConfig, RetrievalConfig
from bedrock_agentcore.memory.integrations.strands.session_manager import AgentCoreMemorySessionManager

app = BedrockAgentCoreApp(debug=True)

# NOTE: No StrandsTelemetry() needed - opentelemetry-instrument sets up global tracer
# Strands will automatically use the global tracer provider from ADOT
logger.info("Strands using ADOT global tracer (set by opentelemetry-instrument)")

GATEWAY_URL = os.environ.get("GATEWAY_URL", "")
MODEL_ID = os.environ.get("MODEL_ID", "us.anthropic.claude-haiku-4-5-20251001-v1:0")
AWS_REGION = os.environ.get("AWS_REGION", "us-east-1")
MEMORY_ID = os.environ.get("MEMORY_ID", "")

logger.info(f"Initializing BedrockModel: model_id={MODEL_ID}, region={AWS_REGION}")
model = BedrockModel(model_id=MODEL_ID)
logger.info("BedrockModel initialized successfully")

logger.info(f"MEMORY_ID={'SET: ' + MEMORY_ID if MEMORY_ID else 'NOT SET'}")

# Pre-connect MCP client at module level — reused across invocations
mcp_client = MCPClient(
    lambda: aws_iam_streamablehttp_client(
        endpoint=GATEWAY_URL,
        aws_region=AWS_REGION,
        aws_service="bedrock-agentcore",
    )
) if GATEWAY_URL else None

if mcp_client:
    logger.info("MCP client configured for Gateway")

SYSTEM_PROMPT = """You are a document intelligence assistant with conversational memory.

You have memory of previous conversations. Information inside <user_context> tags contains recalled facts,
preferences, and summaries from past sessions — treat this as your memory of prior interactions.
Use this memory for conversational context and user preferences only.

CRITICAL RULES:
1. ALWAYS call tools for current document state:
   - "list my documents" → call get_document_metadata
   - "search for X" → call search_documents
   - "compare X and Y" → FIRST call get_document_metadata to get document IDs, THEN call compare_documents
2. NEVER answer questions about documents using only memory.
3. Memory is for user preferences and conversation history, NOT current document inventory.
4. Document state changes between sessions - always fetch fresh data from tools.

Available tools and their EXACT parameters:

1. search_documents
   - query (string, required): The search query
   - limit (integer, optional): Max results to return

2. get_document_metadata
   - document_id (string, optional): Specific document ID to retrieve
   - limit (integer, optional): Max documents to list

3. compare_documents
   - query (string, required): What aspect to compare
   - document_id_a (string, required): First document's ID (UUID)
   - document_id_b (string, required): Second document's ID (UUID)
   - limit (integer, optional): Max matches per document

IMPORTANT for compare_documents:
- document_id_a and document_id_b are UUIDs (e.g., "a1b2c3d4-e5f6-..."), NOT filenames
- You MUST call get_document_metadata FIRST to get the actual document IDs
- Example workflow:
  1. User asks: "compare LEARNING-PLAN and CAREER-STRATEGY"
  2. Call get_document_metadata to get list of documents with their IDs
  3. Find the IDs for documents with those titles
  4. Call compare_documents with the actual UUIDs

NOTE: You only see documents belonging to the authenticated user. Tenant isolation is handled automatically.

Always cite sources with document titles. Be concise and accurate."""


class TenantInjectorMCPClient:
    """Wrapper that injects tenant_id into all MCP tool calls."""

    def __init__(self, tenant_id: str, mcp_client: MCPClient):
        self.tenant_id = tenant_id
        self.mcp_client = mcp_client
        logger.info(f"TenantInjectorMCPClient initialized with tenant_id={tenant_id}")

    async def __call__(self, tool_name: str, tool_input: Dict[str, Any]) -> Any:
        """Intercept tool calls and inject tenant_id automatically."""
        tool_input_with_tenant = {**tool_input, "tenant_id": self.tenant_id}
        logger.info(f"Tool call: {tool_name} with injected tenant_id={self.tenant_id}")
        return await self.mcp_client(tool_name, tool_input_with_tenant)

    def __getattr__(self, name):
        """Delegate all other attributes to the wrapped client."""
        return getattr(self.mcp_client, name)


@app.entrypoint
async def invoke(payload):
    """AgentCore Runtime entry point — streams text tokens back to caller."""
    try:
        query = payload.get("prompt", "")
        tenant_id = payload.get("tenant_id", "tenant-1")
        actor_id = payload.get("actor_id", tenant_id)
        session_id = payload.get("session_id", "default")

        logger.info(f"Invocation: tenant_id={tenant_id}, actor={actor_id}, session={session_id}")

        session_manager = None
        if MEMORY_ID:
            try:
                logger.info(f"Initializing memory: memory_id={MEMORY_ID}, actor={actor_id}, session={session_id}")
                config = AgentCoreMemoryConfig(
                    memory_id=MEMORY_ID,
                    session_id=session_id,
                    actor_id=actor_id,
                    retrieval_config={
                        "/facts/{actorId}/": RetrievalConfig(max_results=5),
                        "/summaries/{actorId}/{sessionId}/": RetrievalConfig(max_results=2),
                        "/preferences/{actorId}/": RetrievalConfig(max_results=3),
                    },
                )
                session_manager = AgentCoreMemorySessionManager(
                    agentcore_memory_config=config,
                    region_name=AWS_REGION,
                )
                logger.info("Memory session manager created successfully")
            except Exception as e:
                logger.warning(f"Memory init failed, continuing without memory: {e}")
        else:
            logger.warning("MEMORY_ID not set, skipping memory")

        # Wrap MCP client with tenant_id injector
        tenant_aware_mcp = None
        if mcp_client:
            tenant_aware_mcp = TenantInjectorMCPClient(tenant_id, mcp_client)

        agent = Agent(
            model=model,
            system_prompt=SYSTEM_PROMPT,
            tools=[tenant_aware_mcp] if tenant_aware_mcp else [],
            session_manager=session_manager,
        )

        try:
            logger.info(f"Starting agent stream for query: {query[:100]}...")
            event_count = 0
            # No longer need to prefix query with tenant_id since it's injected
            async for event in agent.stream_async(query):
                event_count += 1
                if isinstance(event, dict) and "data" in event:
                    yield event["data"]
            logger.info(f"Agent stream completed: {event_count} events yielded")
        finally:
            if session_manager:
                try:
                    session_manager.close()
                except Exception as e:
                    logger.warning(f"Memory flush failed: {e}")
    except Exception as e:
        logger.error(f"Agent error: {e}")
        logger.error(traceback.format_exc())
        raise


if __name__ == "__main__":
    app.run()
