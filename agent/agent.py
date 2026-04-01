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

app = BedrockAgentCoreApp(debug=True)

GATEWAY_URL = os.environ.get("GATEWAY_URL", "")
MODEL_ID = os.environ.get("MODEL_ID", "us.anthropic.claude-haiku-4-5-20251001-v1:0")
AWS_REGION = os.environ.get("AWS_REGION", "us-east-1")

model = BedrockModel(model_id=MODEL_ID)

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

SYSTEM_PROMPT = """You are a document intelligence assistant.

Use search_documents to find information across the document corpus.
Use get_document_metadata to list available documents or get details.
Use compare_documents to compare two documents side-by-side.

Always cite sources with document titles. Be concise and accurate."""

# Pre-build agent once — reused across invocations
agent = Agent(
    model=model,
    system_prompt=SYSTEM_PROMPT,
    tools=[mcp_client] if mcp_client else [],
)


@app.entrypoint
async def invoke(payload):
    """AgentCore Runtime entry point — streams text tokens back to caller."""
    try:
        query = payload.get("prompt", "")
        tenant_id = payload.get("tenant_id", "tenant-1")
        async for event in agent.stream_async(f"[tenant_id={tenant_id}] {query}"):
            if isinstance(event, dict) and "data" in event:
                yield event["data"]
    except Exception as e:
        logger.error(f"Agent error: {e}")
        logger.error(traceback.format_exc())
        raise


if __name__ == "__main__":
    app.run()
