from aws_cdk import (
    Stack,
    CfnOutput,
    Duration,
    aws_iam as iam,
    aws_bedrockagentcore as agentcore,
)
from constructs import Construct
from stacks.agent_stack import AgentStack


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
                    "StringEquals": {
                        "aws:SourceAccount": self.account,
                        "aws:ResourceAccount": self.account,
                    },
                    "ArnLike": {
                        "aws:SourceArn": [
                            f"arn:aws:bedrock-agentcore:{self.region}:{self.account}:evaluator/*",
                            f"arn:aws:bedrock-agentcore:{self.region}:{self.account}:online-evaluation-config/*",
                        ]
                    },
                },
            ),
            description="Execution role for AgentCore evaluations",
        )

        # CloudWatch Logs permissions - read agent runtime logs and write evaluation results
        # Note: Must use wildcard resource per AWS Bedrock requirements
        evaluation_role.add_to_policy(iam.PolicyStatement(
            sid="CloudWatchLogsRead",
            actions=[
                "logs:DescribeLogGroups",
                "logs:GetQueryResults",
                "logs:StartQuery",
            ],
            resources=["*"],
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
                "logs:DescribeIndexPolicies",
                "logs:PutIndexPolicy",
            ],
            resources=[
                f"arn:aws:logs:{self.region}:{self.account}:log-group:aws/spans",
                f"arn:aws:logs:{self.region}:{self.account}:log-group:aws/spans:*",
            ],
        ))

        # Bedrock model invocation for evaluators (if custom evaluators are added later)
        evaluation_role.add_to_policy(iam.PolicyStatement(
            sid="BedrockModelInvocation",
            actions=[
                "bedrock:InvokeModel",
                "bedrock:InvokeModelWithResponseStream",
            ],
            resources=[
                "arn:aws:bedrock:*::foundation-model/-",
                f"arn:aws:bedrock:*:{self.account}:inference-profile/-",
            ],
        ))

        # Environment-specific configuration
        sampling_percentage = 100.0 if environment == "dev" else 10.0
        config_name = f"docint_agent_eval_{environment}"

        # Import runtime from agent stack for evaluation data source
        # Agent stack uses CfnRuntime (L1), need to wrap as Runtime (L2) for evaluation
        imported_runtime = agentcore.Runtime.from_agent_runtime_attributes(
            self, "ImportedRuntime",
            agent_runtime_id=agent_stack.runtime.attr_agent_runtime_id,
            agent_runtime_arn=f"arn:aws:bedrock-agentcore:{self.region}:{self.account}:runtime/{agent_stack.runtime.attr_agent_runtime_id}",
            agent_runtime_name="docint_agent",
            role_arn=agent_stack.runtime.role_arn,
        )

        # Create online evaluation config using L2 construct (native CloudFormation support)
        # Using agent runtime endpoint as data source (more direct than CloudWatch Logs)
        eval_config = agentcore.OnlineEvaluationConfig(
            self, "OnlineEvaluationConfig",
            online_evaluation_config_name=config_name,
            description=f"CDK-managed evaluation for docint agent ({environment})",
            data_source=agentcore.DataSourceConfig.from_agent_runtime_endpoint_name(
                imported_runtime, "docint_agent_endpoint"
            ),
            evaluators=[
                agentcore.EvaluatorSelector.builtin(agentcore.BuiltinEvaluator.CONCISENESS),
                agentcore.EvaluatorSelector.builtin(agentcore.BuiltinEvaluator.CORRECTNESS),
                agentcore.EvaluatorSelector.builtin(agentcore.BuiltinEvaluator.GOAL_SUCCESS_RATE),
                agentcore.EvaluatorSelector.builtin(agentcore.BuiltinEvaluator.INSTRUCTION_FOLLOWING),
                agentcore.EvaluatorSelector.builtin(agentcore.BuiltinEvaluator.TOOL_PARAMETER_ACCURACY),
                agentcore.EvaluatorSelector.builtin(agentcore.BuiltinEvaluator.TOOL_SELECTION_ACCURACY),
            ],
            sampling_percentage=sampling_percentage,
            session_timeout=Duration.minutes(5),
            execution_role=evaluation_role,
            execution_status=agentcore.ExecutionStatus.ENABLED,
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
            self, "EvaluationConfigArn",
            value=eval_config.online_evaluation_config_arn,
            description="Online evaluation config ARN",
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
