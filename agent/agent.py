"""Document Intelligence Agent — runs on AgentCore Runtime."""
import os
import logging
from bedrock_agentcore import BedrockAgentCoreApp
from strands import Agent
from strands.models import BedrockModel
from strands.tools.mcp.mcp_client import MCPClient
from mcp.client.streamable_http import streamablehttp_client

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

app = BedrockAgentCoreApp()

GATEWAY_URL = os.environ.get("GATEWAY_URL", "")
MODEL_ID = os.environ.get("MODEL_ID", "us.anthropic.claude-sonnet-4-20250514-v1:0")

model = BedrockModel(model_id=MODEL_ID)

# Connect to AgentCore Gateway via streamable HTTP (MCP protocol).
# Inside AgentCore Runtime, the execution role credentials are used automatically.
mcp_client = MCPClient(lambda: streamablehttp_client(GATEWAY_URL)) if GATEWAY_URL else None

SYSTEM_PROMPT = """You are a document intelligence assistant.

Use search_documents to find information across the document corpus.
Use get_document_metadata to list available documents or get details.
Use compare_documents to compare two documents side-by-side.

Always cite sources with document titles. Be concise and accurate."""


@app.entrypoint
def invoke(payload):
    """AgentCore Runtime entry point."""
    query = payload.get("prompt", "")
    tenant_id = payload.get("tenant_id", "tenant-1")

    if mcp_client:
        with mcp_client:
            tools = mcp_client.list_tools_sync()
            agent = Agent(model=model, system_prompt=SYSTEM_PROMPT, tools=tools)
            result = agent(f"[tenant_id={tenant_id}] {query}")
    else:
        agent = Agent(model=model, system_prompt=SYSTEM_PROMPT)
        result = agent(f"[tenant_id={tenant_id}] {query}")

    return result.message["content"][0]["text"]


if __name__ == "__main__":
    app.run()
