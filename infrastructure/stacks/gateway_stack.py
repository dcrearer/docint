from aws_cdk import (
    Stack,
    aws_bedrockagentcore as agentcore,
    aws_iam as iam,
)
from constructs import Construct
from stacks.lambda_stack import LambdaStack

Schema = agentcore.CfnGatewayTarget.SchemaDefinitionProperty


def _tool_target(scope, id, *, gateway, lambda_fn, tool_name, description, properties, required):
    """Helper to create a CfnGatewayTarget for a Lambda tool."""
    schema_props = {
        k: Schema(type=v["type"], description=v.get("description"))
        for k, v in properties.items()
    }

    return agentcore.CfnGatewayTarget(
        scope, id,
        name=tool_name,
        description=description,
        gateway_identifier=gateway.attr_gateway_identifier,
        target_configuration=agentcore.CfnGatewayTarget.TargetConfigurationProperty(
            mcp=agentcore.CfnGatewayTarget.McpTargetConfigurationProperty(
                lambda_=agentcore.CfnGatewayTarget.McpLambdaTargetConfigurationProperty(
                    lambda_arn=lambda_fn.function_arn,
                    tool_schema=agentcore.CfnGatewayTarget.ToolSchemaProperty(
                        inline_payload=[
                            agentcore.CfnGatewayTarget.ToolDefinitionProperty(
                                name=tool_name,
                                description=description,
                                input_schema=Schema(
                                    type="object",
                                    properties=schema_props,
                                    required=required,
                                ),
                            )
                        ]
                    ),
                )
            )
        ),
    )


class GatewayStack(Stack):
    def __init__(self, scope: Construct, id: str, lambdas: LambdaStack, **kwargs):
        super().__init__(scope, id, **kwargs)

        gateway_role = iam.Role(
            self, "GatewayRole",
            assumed_by=iam.ServicePrincipal("bedrock-agentcore.amazonaws.com"),
        )
        for fn in [lambdas.search_fn, lambdas.metadata_fn, lambdas.compare_fn]:
            fn.grant_invoke(gateway_role)

        self.gateway = agentcore.CfnGateway(
            self, "Gateway",
            name="docint-gateway",
            description="Document intelligence MCP gateway",
            authorizer_type="AWS_IAM",
            protocol_type="MCP",
            role_arn=gateway_role.role_arn,
        )

        _tool_target(
            self, "SearchTarget",
            gateway=self.gateway,
            lambda_fn=lambdas.search_fn,
            tool_name="search-documents",
            description="Hybrid vector + full-text search over documents",
            properties={
                "query": {"type": "string", "description": "Search query text"},
                "tenant_id": {"type": "string", "description": "Tenant identifier"},
                "limit": {"type": "integer", "description": "Max results to return"},
            },
            required=["query", "tenant_id"],
        )

        _tool_target(
            self, "MetadataTarget",
            gateway=self.gateway,
            lambda_fn=lambdas.metadata_fn,
            tool_name="get-document-metadata",
            description="List documents or get metadata for a specific document",
            properties={
                "tenant_id": {"type": "string", "description": "Tenant identifier"},
                "document_id": {"type": "string", "description": "Optional document ID"},
                "limit": {"type": "integer", "description": "Max documents to list"},
            },
            required=["tenant_id"],
        )

        _tool_target(
            self, "CompareTarget",
            gateway=self.gateway,
            lambda_fn=lambdas.compare_fn,
            tool_name="compare-documents",
            description="Compare two documents side-by-side for a query",
            properties={
                "query": {"type": "string", "description": "Comparison query"},
                "document_id_a": {"type": "string", "description": "First document ID"},
                "document_id_b": {"type": "string", "description": "Second document ID"},
                "tenant_id": {"type": "string", "description": "Tenant identifier"},
                "limit": {"type": "integer", "description": "Max matches per document"},
            },
            required=["query", "document_id_a", "document_id_b", "tenant_id"],
        )
