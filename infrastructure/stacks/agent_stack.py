from aws_cdk import (
    Stack,
    CfnOutput,
    aws_bedrockagentcore as agentcore,
    aws_iam as iam,
    aws_s3_assets as s3_assets,
)
from constructs import Construct
from stacks.gateway_stack import GatewayStack


class AgentStack(Stack):
    def __init__(self, scope: Construct, id: str, gateway: GatewayStack, **kwargs):
        super().__init__(scope, id, **kwargs)

        # Role for the agent runtime
        role = iam.Role(
            self, "AgentRole",
            assumed_by=iam.ServicePrincipal("bedrock-agentcore.amazonaws.com"),
        )
        role.add_to_policy(iam.PolicyStatement(
            actions=["bedrock:InvokeModel"],
            resources=["arn:aws:bedrock:*::foundation-model/*"],
        ))
        role.add_to_policy(iam.PolicyStatement(
            actions=["bedrock-agentcore:Invoke*", "bedrock-agentcore:GetGateway"],
            resources=["*"],
        ))

        # Package agent code as S3 asset
        agent_code = s3_assets.Asset(self, "AgentCode", path="../agent")

        # AgentCore Runtime
        runtime = agentcore.CfnRuntime(
            self, "Runtime",
            agent_runtime_name="docint_agent",
            description="Document intelligence agent with Claude Sonnet",
            role_arn=role.role_arn,
            agent_runtime_artifact=agentcore.CfnRuntime.AgentRuntimeArtifactProperty(
                code_configuration=agentcore.CfnRuntime.CodeConfigurationProperty(
                    runtime="PYTHON_3_13",
                    entry_point=["agent", "handler"],
                    code=agentcore.CfnRuntime.CodeProperty(
                        s3=agentcore.CfnRuntime.S3LocationProperty(
                            bucket=agent_code.s3_bucket_name,
                            prefix=agent_code.s3_object_key,
                        )
                    ),
                )
            ),
            environment_variables={
                "GATEWAY_URL": gateway.gateway.attr_gateway_url,
                "MODEL_ID": "anthropic.claude-sonnet-4-20250514-v1:0",
            },
            network_configuration=agentcore.CfnRuntime.NetworkConfigurationProperty(
                network_mode="PUBLIC",
            ),
        )

        # Runtime endpoint
        endpoint = agentcore.CfnRuntimeEndpoint(
            self, "Endpoint",
            name="docint_agent_endpoint",
            description="Production endpoint for document intelligence agent",
            agent_runtime_id=runtime.attr_agent_runtime_id,
        )

        CfnOutput(self, "RuntimeId", value=runtime.attr_agent_runtime_id)
        CfnOutput(self, "EndpointArn", value=endpoint.attr_agent_runtime_endpoint_arn)
