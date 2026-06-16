from aws_cdk import (
    Stack,
    CfnOutput,
    aws_iam as iam,
    aws_logs as logs,
    CustomResource,
    custom_resources as cr,
)
from constructs import Construct
from stacks.agent_stack import AgentStack
import json


class EvaluationStack(Stack):
    def __init__(
        self,
        scope: Construct,
        id: str,
        agent_stack: AgentStack,
        environment: str = "dev",
        **kwargs
    ):
        super().__init__(scope, id, **kwargs)

        # IAM role for evaluation service execution
        evaluation_role = iam.Role(
            self, "EvaluationExecutionRole",
            assumed_by=iam.PrincipalWithConditions(
                iam.ServicePrincipal("bedrock-agentcore.amazonaws.com"),
                conditions={
                    "StringEquals": {"aws:SourceAccount": self.account},
                    "ArnLike": {
                        "aws:SourceArn": f"arn:aws:bedrock-agentcore:{self.region}:{self.account}:online-evaluation-config/*"
                    },
                },
            ),
            description="Execution role for AgentCore evaluations",
        )

        # CloudWatch Logs permissions - read agent runtime logs and write evaluation results
        evaluation_role.add_to_policy(iam.PolicyStatement(
            sid="CloudWatchLogsRead",
            actions=[
                "logs:DescribeLogGroups",
                "logs:DescribeLogStreams",
                "logs:GetLogEvents",
                "logs:FilterLogEvents",
            ],
            resources=[
                f"arn:aws:logs:{self.region}:{self.account}:log-group:/aws/bedrock-agentcore/runtimes/*",
                f"arn:aws:logs:{self.region}:{self.account}:log-group:aws/spans:*",
            ],
        ))

        evaluation_role.add_to_policy(iam.PolicyStatement(
            sid="CloudWatchLogsWrite",
            actions=[
                "logs:CreateLogGroup",
                "logs:CreateLogStream",
                "logs:PutLogEvents",
            ],
            resources=[
                f"arn:aws:logs:{self.region}:{self.account}:log-group:/aws/bedrock-agentcore/evaluations/*",
            ],
        ))

        # CloudWatch Logs indexing for Transaction Search
        evaluation_role.add_to_policy(iam.PolicyStatement(
            sid="CloudWatchIndexing",
            actions=[
                "logs:PutIndexPolicy",
                "logs:GetIndexPolicy",
            ],
            resources=[
                f"arn:aws:logs:{self.region}:{self.account}:log-group:aws/spans:*",
            ],
        ))

        # Bedrock model invocation for evaluators (if custom evaluators are added later)
        evaluation_role.add_to_policy(iam.PolicyStatement(
            sid="BedrockModelInvocation",
            actions=["bedrock:InvokeModel"],
            resources=[
                "arn:aws:bedrock:*::foundation-model/*",
            ],
        ))

        # Environment-specific configuration
        sampling_percentage = 100.0 if environment == "dev" else 10.0
        config_name = f"docint_agent_eval_{environment}"

        # Get agent runtime log group name from agent stack outputs
        # Format: /aws/bedrock-agentcore/runtimes/{runtime_name}-{id}-{endpoint_name}
        agent_log_group = f"/aws/bedrock-agentcore/runtimes/docint_agent-lsc56PDJsX-docint_agent_endpoint"
        service_name = "docint_agent.docint_agent_endpoint"

        # Create online evaluation config using Custom Resource
        # Note: CDK doesn't have L1/L2 constructs for bedrock-agentcore evaluations yet
        eval_config = cr.AwsCustomResource(
            self, "OnlineEvaluationConfig",
            on_create=cr.AwsSdkCall(
                service="BedrockAgentCoreControl",
                action="createOnlineEvaluationConfig",
                parameters={
                    "onlineEvaluationConfigName": config_name,
                    "description": f"CDK-managed evaluation for docint agent ({environment})",
                    "dataSourceConfig": {
                        "cloudWatchLogs": {
                            "logGroupNames": [agent_log_group],
                            "serviceNames": [service_name],
                        }
                    },
                    "evaluators": [
                        {"evaluatorId": "Builtin.Conciseness"},
                        {"evaluatorId": "Builtin.Correctness"},
                        {"evaluatorId": "Builtin.GoalSuccessRate"},
                        {"evaluatorId": "Builtin.InstructionFollowing"},
                        {"evaluatorId": "Builtin.ToolParameterAccuracy"},
                        {"evaluatorId": "Builtin.ToolSelectionAccuracy"},
                    ],
                    "rule": {
                        "samplingConfig": {
                            "samplingPercentage": sampling_percentage,
                        },
                        "sessionConfig": {
                            "sessionTimeoutMinutes": 5,
                        },
                    },
                    "evaluationExecutionRoleArn": evaluation_role.role_arn,
                    "enableOnCreate": True,
                },
                physical_resource_id=cr.PhysicalResourceId.from_response("onlineEvaluationConfigId"),
            ),
            on_update=cr.AwsSdkCall(
                service="BedrockAgentCoreControl",
                action="updateOnlineEvaluationConfig",
                parameters={
                    "onlineEvaluationConfigId": cr.PhysicalResourceIdReference(),
                    "evaluators": [
                        {"evaluatorId": "Builtin.Conciseness"},
                        {"evaluatorId": "Builtin.Correctness"},
                        {"evaluatorId": "Builtin.GoalSuccessRate"},
                        {"evaluatorId": "Builtin.InstructionFollowing"},
                        {"evaluatorId": "Builtin.ToolParameterAccuracy"},
                        {"evaluatorId": "Builtin.ToolSelectionAccuracy"},
                    ],
                    "rule": {
                        "samplingConfig": {
                            "samplingPercentage": sampling_percentage,
                        },
                        "sessionConfig": {
                            "sessionTimeoutMinutes": 5,
                        },
                    },
                    "evaluationExecutionRoleArn": evaluation_role.role_arn,
                    "executionStatus": "ENABLED",
                },
            ),
            on_delete=cr.AwsSdkCall(
                service="BedrockAgentCoreControl",
                action="deleteOnlineEvaluationConfig",
                parameters={
                    "onlineEvaluationConfigId": cr.PhysicalResourceIdReference(),
                },
            ),
            policy=cr.AwsCustomResourcePolicy.from_statements([
                iam.PolicyStatement(
                    actions=[
                        "bedrock-agentcore-control:CreateOnlineEvaluationConfig",
                        "bedrock-agentcore-control:UpdateOnlineEvaluationConfig",
                        "bedrock-agentcore-control:DeleteOnlineEvaluationConfig",
                        "bedrock-agentcore-control:GetOnlineEvaluationConfig",
                    ],
                    resources=["*"],
                ),
                iam.PolicyStatement(
                    actions=["iam:PassRole"],
                    resources=[evaluation_role.role_arn],
                ),
            ]),
        )

        # Ensure role exists before creating evaluation config
        eval_config.node.add_dependency(evaluation_role)
        eval_config.node.add_dependency(agent_stack)

        # Outputs
        CfnOutput(
            self, "EvaluationRoleArn",
            value=evaluation_role.role_arn,
            description="IAM role for evaluation execution",
        )

        CfnOutput(
            self, "EvaluationConfigId",
            value=eval_config.get_response_field("onlineEvaluationConfigId"),
            description="Online evaluation config ID",
        )

        CfnOutput(
            self, "EvaluationConfigName",
            value=config_name,
            description="Online evaluation config name",
        )

        CfnOutput(
            self, "SamplingRate",
            value=str(sampling_percentage),
            description="Evaluation sampling percentage",
        )

        # Store for cross-stack references
        self.evaluation_role = evaluation_role
        self.eval_config = eval_config
