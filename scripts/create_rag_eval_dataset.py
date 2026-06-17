#!/usr/bin/env python3
"""
Build a Bedrock RAG evaluation dataset using "bring your own inference".

For a custom RAG system (not a Bedrock Knowledge Base), Bedrock does NOT call
your retrieval pipeline. Instead, YOU run retrieval ahead of time and embed the
results in the dataset. Bedrock acts only as the judge that scores those
pre-computed results.

This script:
  1. Reads a queries file (query + ground-truth answer you author by hand)
  2. Invokes the docint-search Lambda for each query (the "inference" step)
  3. Writes a JSONL dataset in the exact `conversationTurns` format the
     Bedrock RAG evaluation API expects for retrieve-only jobs.

Dataset format produced (one JSON object per line):
    {
      "conversationTurns": [{
        "prompt": {"content": [{"text": "<query>"}]},
        "referenceResponses": [{"content": [{"text": "<ground-truth answer>"}]}],
        "output": {
          "knowledgeBaseIdentifier": "<rag-source-id>",
          "retrievedResults": {
            "retrievalResults": [
              {"content": {"text": "<chunk content>"},
               "metadata": {"chunk_id": "...", "document_id": "...",
                            "title": "...", "distance": "..."}}
            ]
          }
        }
      }]
    }

`referenceResponses` is required for the Context coverage metric.
`output.retrievedResults` is the "bring your own inference" payload — the chunks
docint-search actually returned.

------------------------------------------------------------------------------
INPUT: queries file (JSON)
------------------------------------------------------------------------------
A JSON list. Each item needs a `query` and a `reference_answer`. `tenant_id`,
`limit`, and `category` are optional.

    [
      {
        "query": "How do I deploy a Lambda with CDK?",
        "reference_answer": "Use the aws_lambda.Function construct and run cdk deploy.",
        "tenant_id": "default-tenant",
        "limit": 5,
        "category": "content_search"
      }
    ]

Use --emit-template to write a starter queries file you can edit.

------------------------------------------------------------------------------
USAGE
------------------------------------------------------------------------------
    # 1. Write a starter queries file and edit it
    python scripts/create_rag_eval_dataset.py --emit-template queries.json

    # 2. Build the dataset (invokes docint-search for each query)
    python scripts/create_rag_eval_dataset.py \
        --queries queries.json \
        --output datasets/retrieve-only-v1.jsonl

    # 3. Upload to the dataset bucket
    aws s3 cp datasets/retrieve-only-v1.jsonl \
        s3://docint-rag-eval-datasets-prod-<account-id>/retrieve-only-v1.jsonl
"""

import argparse
import json
import sys
from pathlib import Path

import boto3
from botocore.exceptions import ClientError


# Default search Lambda name (override with --function-name)
DEFAULT_FUNCTION_NAME = "docint-search"
# Label that identifies this RAG source in the evaluation results
DEFAULT_RAG_SOURCE_ID = "docint-hybrid-search"


TEMPLATE_QUERIES = [
    {
        "query": "How do I deploy a Lambda function with CDK?",
        "reference_answer": "Use the aws_lambda.Function construct in your stack and run cdk deploy.",
        "tenant_id": "default-tenant",
        "limit": 5,
        "category": "content_search",
    },
    {
        "query": "What documents do I have about vector search?",
        "reference_answer": "Replace with the real expected answer for your corpus.",
        "tenant_id": "default-tenant",
        "limit": 5,
        "category": "metadata_query",
    },
    {
        "query": "Show me documents about a topic that does not exist",
        "reference_answer": "No relevant documents were found.",
        "tenant_id": "default-tenant",
        "limit": 5,
        "category": "edge_case",
    },
]


def emit_template(path: Path) -> None:
    """Write a starter queries file the user can edit."""
    if path.exists():
        print(f"ERROR: {path} already exists — refusing to overwrite.", file=sys.stderr)
        sys.exit(1)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(TEMPLATE_QUERIES, indent=2) + "\n")
    print(f"Wrote starter queries file: {path}")
    print("Edit it (real queries + ground-truth answers), then run with --queries.")


def load_queries(path: Path) -> list:
    """Load and validate the queries file."""
    try:
        data = json.loads(path.read_text())
    except FileNotFoundError:
        print(f"ERROR: queries file not found: {path}", file=sys.stderr)
        sys.exit(1)
    except json.JSONDecodeError as e:
        print(f"ERROR: queries file is not valid JSON: {e}", file=sys.stderr)
        sys.exit(1)

    if not isinstance(data, list) or not data:
        print("ERROR: queries file must be a non-empty JSON list.", file=sys.stderr)
        sys.exit(1)

    for i, item in enumerate(data):
        if "query" not in item or not item["query"]:
            print(f"ERROR: item {i} is missing 'query'.", file=sys.stderr)
            sys.exit(1)
        if "reference_answer" not in item or not item["reference_answer"]:
            print(
                f"ERROR: item {i} ('{item.get('query', '')[:40]}') is missing "
                "'reference_answer' (required for the Context coverage metric).",
                file=sys.stderr,
            )
            sys.exit(1)
    return data


def invoke_search(client, function_name: str, query: str, tenant_id: str, limit: int) -> list:
    """Invoke docint-search and return its `results` list.

    Raises on transport error; returns [] if the function returns no results.
    """
    payload = {"query": query, "limit": limit}
    if tenant_id:
        payload["tenant_id"] = tenant_id

    resp = client.invoke(
        FunctionName=function_name,
        InvocationType="RequestResponse",
        Payload=json.dumps(payload).encode("utf-8"),
    )

    body = resp["Payload"].read().decode("utf-8")

    # A Lambda function error surfaces as FunctionError + an error envelope in body.
    if resp.get("FunctionError"):
        raise RuntimeError(f"search Lambda returned FunctionError: {body}")

    parsed = json.loads(body)
    return parsed.get("results", [])


def to_conversation_turn(item: dict, results: list, rag_source_id: str) -> dict:
    """Build one `conversationTurns` record in the Bedrock RAG eval format."""
    retrieval_results = []
    for r in results:
        retrieval_results.append({
            "content": {"text": r.get("content", "")},
            # metadata values must be strings
            "metadata": {
                "chunk_id": str(r.get("chunk_id", "")),
                "document_id": str(r.get("document_id", "")),
                "title": str(r.get("title", "")),
                "distance": str(r.get("distance", "")),
            },
        })

    return {
        "conversationTurns": [{
            "prompt": {"content": [{"text": item["query"]}]},
            "referenceResponses": [
                {"content": [{"text": item["reference_answer"]}]}
            ],
            "output": {
                "knowledgeBaseIdentifier": rag_source_id,
                "retrievedResults": {"retrievalResults": retrieval_results},
            },
        }]
    }


def build_dataset(
    queries: list,
    output_path: Path,
    function_name: str,
    rag_source_id: str,
    region: str,
    default_tenant: str,
    default_limit: int,
) -> None:
    client = boto3.client("lambda", region_name=region)
    output_path.parent.mkdir(parents=True, exist_ok=True)

    written = 0
    empty_retrievals = 0

    with open(output_path, "w") as f:
        for i, item in enumerate(queries):
            tenant_id = item.get("tenant_id", default_tenant)
            limit = int(item.get("limit", default_limit))
            query = item["query"]

            try:
                results = invoke_search(client, function_name, query, tenant_id, limit)
            except ClientError as e:
                print(
                    f"ERROR invoking {function_name} for item {i} "
                    f"('{query[:40]}'): {e}",
                    file=sys.stderr,
                )
                sys.exit(1)
            except RuntimeError as e:
                print(f"ERROR for item {i} ('{query[:40]}'): {e}", file=sys.stderr)
                sys.exit(1)

            if not results:
                empty_retrievals += 1
                print(f"  [warn] item {i} ('{query[:40]}') returned 0 chunks")

            record = to_conversation_turn(item, results, rag_source_id)
            f.write(json.dumps(record) + "\n")
            written += 1
            print(f"  [{written}/{len(queries)}] '{query[:50]}' -> {len(results)} chunks")

    print()
    print(f"Wrote {written} prompts to {output_path}")
    if empty_retrievals:
        print(
            f"NOTE: {empty_retrievals} queries returned 0 chunks. Edge-case queries "
            "may be intentional; otherwise check the query or tenant_id."
        )
    print()
    print("Next steps:")
    print(f"  1. Review {output_path} (one JSON object per line)")
    print("  2. Upload to the dataset bucket:")
    print(f"       aws s3 cp {output_path} s3://<dataset-bucket>/{output_path.name}")
    print("  3. Create the eval job in the AWS Console:")
    print("       Bedrock -> Evaluations -> Create -> RAG evaluation")
    print("       Type: Retrieve only | Inference source: Bring your own inference response")
    print(f"       Dataset S3 URI: s3://<dataset-bucket>/{output_path.name}")
    print("     (See docs/RAG-EVALUATION.md for the full job-creation walkthrough.)")


def main():
    parser = argparse.ArgumentParser(
        description="Build a Bedrock RAG evaluation dataset by invoking docint-search.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--emit-template",
        type=Path,
        metavar="PATH",
        help="Write a starter queries file to PATH and exit.",
    )
    parser.add_argument(
        "--queries",
        type=Path,
        help="Path to the queries JSON file (see --emit-template).",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("datasets/retrieve-only-v1.jsonl"),
        help="Output JSONL dataset path (default: datasets/retrieve-only-v1.jsonl).",
    )
    parser.add_argument(
        "--function-name",
        default=DEFAULT_FUNCTION_NAME,
        help=f"Search Lambda name (default: {DEFAULT_FUNCTION_NAME}).",
    )
    parser.add_argument(
        "--rag-source-id",
        default=DEFAULT_RAG_SOURCE_ID,
        help=f"Label for the RAG source in results (default: {DEFAULT_RAG_SOURCE_ID}).",
    )
    parser.add_argument(
        "--region",
        default="us-east-1",
        help="AWS region (default: us-east-1).",
    )
    parser.add_argument(
        "--tenant-id",
        default="default-tenant",
        help="Default tenant_id when a query omits it (default: default-tenant).",
    )
    parser.add_argument(
        "--limit",
        type=int,
        default=5,
        help="Default result limit when a query omits it (default: 5).",
    )
    args = parser.parse_args()

    if args.emit_template:
        emit_template(args.emit_template)
        return

    if not args.queries:
        parser.error("either --emit-template or --queries is required")

    queries = load_queries(args.queries)
    build_dataset(
        queries=queries,
        output_path=args.output,
        function_name=args.function_name,
        rag_source_id=args.rag_source_id,
        region=args.region,
        default_tenant=args.tenant_id,
        default_limit=args.limit,
    )


if __name__ == "__main__":
    main()
