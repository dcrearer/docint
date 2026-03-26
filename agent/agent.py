"""Document Intelligence Agent — runs on AgentCore Runtime."""
import os
import json
import boto3
from strands import Agent
from strands.models import BedrockModel

GATEWAY_URL = os.environ["GATEWAY_URL"]
MODEL_ID = os.environ.get("MODEL_ID", "anthropic.claude-sonnet-4-20250514-v1:0")

model = BedrockModel(model_id=MODEL_ID)

agent = Agent(
    model=model,
    system_prompt="""You are a document intelligence assistant.

Use search_documents to find information across the document corpus.
Use get_document_metadata to list available documents or get details.
Use compare_documents to compare two documents side-by-side.

Always cite sources with document titles.
Be concise and accurate.""",
    tools=[GATEWAY_URL],
)


def handler(event, context):
    """Lambda-style handler for AgentCore Runtime."""
    query = event.get("query", "")
    tenant_id = event.get("tenant_id", "tenant-1")

    result = agent(f"[tenant_id={tenant_id}] {query}")
    return {"response": str(result)}
