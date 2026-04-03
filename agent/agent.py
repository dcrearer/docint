"""Document Intelligence Agent — runs on AgentCore Runtime."""
import os
import sys
import logging
import traceback

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

GATEWAY_URL = os.environ.get("GATEWAY_URL", "")
MODEL_ID = os.environ.get("MODEL_ID", "us.anthropic.claude-haiku-4-5-20251001-v1:0")
AWS_REGION = os.environ.get("AWS_REGION", "us-east-1")
MEMORY_ID = os.environ.get("MEMORY_ID", "")

model = BedrockModel(model_id=MODEL_ID)

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

You remember facts, preferences, and summaries from previous conversations with the user.
When context from past conversations is provided, use it naturally — do not claim you have no memory.

Use search_documents to find information across the document corpus.
Use get_document_metadata to list available documents or get details.
Use compare_documents to compare two documents side-by-side.

Always cite sources with document titles. Be concise and accurate."""


@app.entrypoint
async def invoke(payload):
    """AgentCore Runtime entry point — streams text tokens back to caller."""
    try:
        query = payload.get("prompt", "")
        tenant_id = payload.get("tenant_id", "tenant-1")
        actor_id = payload.get("actor_id", tenant_id)
        session_id = payload.get("session_id", "default")

        session_manager = None
        if MEMORY_ID:
            try:
                logger.info(f"Initializing memory: memory_id={MEMORY_ID}, actor={actor_id}, session={session_id}")
                config = AgentCoreMemoryConfig(
                    memory_id=MEMORY_ID,
                    session_id=session_id,
                    actor_id=actor_id,
                    retrieval_config={
                        "/facts/{actorId}/": RetrievalConfig(),
                        "/summaries/{actorId}/{sessionId}/": RetrievalConfig(),
                        "/preferences/{actorId}/": RetrievalConfig(),
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

        agent = Agent(
            model=model,
            system_prompt=SYSTEM_PROMPT,
            tools=[mcp_client] if mcp_client else [],
            session_manager=session_manager,
        )

        try:
            async for event in agent.stream_async(f"[tenant_id={tenant_id}] {query}"):
                if isinstance(event, dict) and "data" in event:
                    yield event["data"]
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
