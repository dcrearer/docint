from aws_cdk import (
    Stack,
    CfnOutput,
    aws_bedrockagentcore as agentcore,
    aws_iam as iam,
    aws_ecr_assets as ecr_assets,
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
        role.add_to_policy(iam.PolicyStatement(
            actions=["ecr:GetAuthorizationToken"],
            resources=["*"],
        ))
        role.add_to_policy(iam.PolicyStatement(
            actions=["ecr:BatchGetImage", "ecr:GetDownloadUrlForLayer"],
            resources=[f"arn:aws:ecr:{self.region}:{self.account}:repository/cdk-*"],
        ))

        # Build and push agent container to ECR
        image = ecr_assets.DockerImageAsset(
            self, "AgentImage",
            directory="../agent",
        )

        # AgentCore Runtime with container deployment
        runtime = agentcore.CfnRuntime(
            self, "Runtime",
            agent_runtime_name="docint_agent",
            description="Document intelligence agent with Claude Sonnet",
            role_arn=role.role_arn,
            agent_runtime_artifact=agentcore.CfnRuntime.AgentRuntimeArtifactProperty(
                container_configuration=agentcore.CfnRuntime.ContainerConfigurationProperty(
                    container_uri=image.image_uri,
                ),
            ),
            environment_variables={
                "GATEWAY_URL": gateway.gateway.attr_gateway_url,
                "MODEL_ID": "us.anthropic.claude-sonnet-4-20250514-v1:0",
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
