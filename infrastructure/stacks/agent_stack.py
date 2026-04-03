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

        # Trust policy per AgentCore docs
        role = iam.Role(
            self, "AgentRole",
            assumed_by=iam.PrincipalWithConditions(
                iam.ServicePrincipal("bedrock-agentcore.amazonaws.com"),
                conditions={
                    "StringEquals": {"aws:SourceAccount": self.account},
                    "ArnLike": {"aws:SourceArn": f"arn:aws:bedrock-agentcore:{self.region}:{self.account}:*"},
                },
            ),
        )

        # Bedrock model invocation
        role.add_to_policy(iam.PolicyStatement(
            actions=["bedrock:InvokeModel", "bedrock:InvokeModelWithResponseStream"],
            resources=[
                "arn:aws:bedrock:*::foundation-model/*",
                f"arn:aws:bedrock:{self.region}:{self.account}:inference-profile/*",
            ],
        ))

        # Gateway access
        role.add_to_policy(iam.PolicyStatement(
            actions=["bedrock-agentcore:Invoke*", "bedrock-agentcore:GetGateway"],
            resources=["*"],
        ))

        # Logs
        role.add_to_policy(iam.PolicyStatement(
            actions=["logs:CreateLogGroup", "logs:DescribeLogStreams"],
            resources=[f"arn:aws:logs:{self.region}:{self.account}:log-group:/aws/bedrock-agentcore/runtimes/*"],
        ))
        role.add_to_policy(iam.PolicyStatement(
            actions=["logs:DescribeLogGroups"],
            resources=[f"arn:aws:logs:{self.region}:{self.account}:log-group:*"],
        ))
        role.add_to_policy(iam.PolicyStatement(
            actions=["logs:CreateLogStream", "logs:PutLogEvents"],
            resources=[f"arn:aws:logs:{self.region}:{self.account}:log-group:/aws/bedrock-agentcore/runtimes/*:log-stream:*"],
        ))

        # AgentCore Memory data plane
        role.add_to_policy(iam.PolicyStatement(
            actions=[
                "bedrock-agentcore:CreateEvent",
                "bedrock-agentcore:RetrieveMemory",
                "bedrock-agentcore:GetSession",
                "bedrock-agentcore:ListSessions",
            ],
            resources=[f"arn:aws:bedrock-agentcore:{self.region}:{self.account}:memory/*"],
        ))

        # Build and push agent container to ECR
        image = ecr_assets.DockerImageAsset(
            self, "AgentImage",
            directory="../agent",
            platform=ecr_assets.Platform.LINUX_ARM64,
        )
        image.repository.grant_pull(role)

        # grant_pull doesn't include GetAuthorizationToken (global action)
        role.add_to_policy(iam.PolicyStatement(
            actions=["ecr:GetAuthorizationToken"],
            resources=["*"],
        ))

        # AgentCore Memory — semantic strategy for cross-session recall, 30-day event expiry
        memory = agentcore.CfnMemory(
            self, "Memory",
            name="docint_memory",
            description="Conversational memory for document intelligence agent",
            event_expiry_duration=30,
            memory_strategies=[
                agentcore.CfnMemory.MemoryStrategyProperty(
                    semantic_memory_strategy=agentcore.CfnMemory.SemanticMemoryStrategyProperty(
                        name="FactExtractor",
                        namespaces=["/facts/{actorId}/"],
                    ),
                ),
                agentcore.CfnMemory.MemoryStrategyProperty(
                    summary_memory_strategy=agentcore.CfnMemory.SummaryMemoryStrategyProperty(
                        name="SessionSummarizer",
                        namespaces=["/summaries/{actorId}/{sessionId}/"],
                    ),
                ),
                agentcore.CfnMemory.MemoryStrategyProperty(
                    user_preference_memory_strategy=agentcore.CfnMemory.UserPreferenceMemoryStrategyProperty(
                        name="PreferenceLearner",
                        namespaces=["/preferences/{actorId}/"],
                    ),
                ),
            ],
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
                "MODEL_ID": "us.anthropic.claude-haiku-4-5-20251001-v1:0",
                "MEMORY_ID": memory.attr_memory_id,
            },
            network_configuration=agentcore.CfnRuntime.NetworkConfigurationProperty(
                network_mode="PUBLIC",
            ),
        )

        # Ensure IAM policy propagates before Runtime validates ECR access
        runtime.node.add_dependency(role)

        # Runtime endpoint
        endpoint = agentcore.CfnRuntimeEndpoint(
            self, "Endpoint",
            name="docint_agent_endpoint",
            description="Production endpoint for document intelligence agent",
            agent_runtime_id=runtime.attr_agent_runtime_id,
        )

        CfnOutput(self, "RuntimeId", value=runtime.attr_agent_runtime_id)
        CfnOutput(self, "EndpointArn", value=endpoint.attr_agent_runtime_endpoint_arn)
        CfnOutput(self, "MemoryId", value=memory.attr_memory_id)
