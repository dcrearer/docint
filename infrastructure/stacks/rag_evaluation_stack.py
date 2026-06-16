"""
RAG Evaluation Stack for Document Intelligence Agent

This stack provides infrastructure for Bedrock RAG (Retrieval-Augmented Generation)
evaluation to measure and optimize search quality. Complements AgentCore evaluations
by providing RAG-specific metrics on retrieval quality, faithfulness, and context relevance.

Architecture:
- Custom RAG implementation (not Bedrock Knowledge Bases)
- "Bring Your Own Inference" pattern via docint-search Lambda
- Evaluate retrieval quality independently of agent behavior

Key Metrics:
- Context Relevance: Are retrieved chunks on-topic?
- Context Coverage: Do chunks contain sufficient information?
- Faithfulness: Does agent stay true to retrieved documents?
- Correctness: Factually accurate responses?
- Completeness: Addresses full question?

Cost Estimate: ~$1.25-2.50 per evaluation run (30 queries)
Monthly Budget: ~$17.50-60 for development and weekly runs

Note: AWS Bedrock does not yet provide native CloudFormation resources for
CreateEvaluationJob API. This stack creates the required infrastructure (IAM, S3,
CloudWatch) and provides helper scripts for manual job creation via AWS CLI.

References:
- TODO-RAG-EVALUATION.md: Complete implementation plan
- evaluation_stack.py: AgentCore evaluation pattern (for agent behavior)
"""

from aws_cdk import (
    CfnOutput,
    Duration,
    RemovalPolicy,
    Stack,
)
from aws_cdk import (
    aws_cloudwatch as cloudwatch,
)
from aws_cdk import (
    aws_cloudwatch_actions as cw_actions,
)
from aws_cdk import (
    aws_iam as iam,
)
from aws_cdk import (
    aws_s3 as s3,
)
from aws_cdk import (
    aws_sns as sns,
)
from constructs import Construct

from stacks.lambda_stack import LambdaStack


class RagEvaluationStack(Stack):
    def __init__(
        self,
        scope: Construct,
        id: str,
        lambda_stack: LambdaStack,
        environment: str = "dev",
        **kwargs,
    ):
        super().__init__(scope, id, **kwargs)

        env_suffix = f"-{environment}" if environment else ""

        # ============================================
        # S3 Buckets for Evaluation Data
        # ============================================

        # Dataset bucket: Stores evaluation queries + ground truth
        # Format: JSONL files with query, expected_chunks, reference_answer
        self.dataset_bucket = s3.Bucket(
            self,
            "DatasetBucket",
            bucket_name=f"docint-rag-eval-datasets{env_suffix}-{self.account}",
            encryption=s3.BucketEncryption.S3_MANAGED,
            versioned=True,  # Track dataset versions for reproducibility
            lifecycle_rules=[
                s3.LifecycleRule(
                    id="ArchiveOldVersions",
                    noncurrent_version_expiration=Duration.days(90),
                    enabled=True,
                )
            ],
            block_public_access=s3.BlockPublicAccess.BLOCK_ALL,
            enforce_ssl=True,
            removal_policy=RemovalPolicy.RETAIN,  # Protect evaluation data
        )

        # Results bucket: Stores evaluation outputs and metrics
        self.results_bucket = s3.Bucket(
            self,
            "ResultsBucket",
            bucket_name=f"docint-rag-eval-results{env_suffix}-{self.account}",
            encryption=s3.BucketEncryption.S3_MANAGED,
            versioned=False,
            lifecycle_rules=[
                s3.LifecycleRule(
                    id="ArchiveOldResults",
                    transitions=[
                        s3.Transition(
                            storage_class=s3.StorageClass.INTELLIGENT_TIERING,
                            transition_after=Duration.days(30),
                        )
                    ],
                    expiration=Duration.days(365),  # Keep 1 year of history
                    enabled=True,
                )
            ],
            block_public_access=s3.BlockPublicAccess.BLOCK_ALL,
            enforce_ssl=True,
            removal_policy=RemovalPolicy.RETAIN,
        )

        # ============================================
        # IAM Role for RAG Evaluation Execution
        # ============================================

        self.evaluation_role = iam.Role(
            self,
            "RagEvaluationExecutionRole",
            assumed_by=iam.ServicePrincipal("bedrock.amazonaws.com"),
            description=f"Execution role for Bedrock RAG evaluations ({environment})",
        )

        # S3 permissions: Read datasets, write results
        self.dataset_bucket.grant_read(self.evaluation_role)
        self.results_bucket.grant_write(self.evaluation_role)

        # Bedrock model invocation for judge models (Claude Sonnet 4.0 or Haiku 4.5)
        self.evaluation_role.add_to_policy(
            iam.PolicyStatement(
                sid="BedrockModelInvocationForJudge",
                actions=[
                    "bedrock:InvokeModel",
                    "bedrock:InvokeModelWithResponseStream",
                ],
                resources=[
                    # Judge model: Claude Sonnet 4.0 (best quality)
                    f"arn:aws:bedrock:{self.region}::foundation-model/anthropic.claude-sonnet-4-20250514-v1:0",
                    # Alternative judge: Claude Haiku 4.5 (cost-effective)
                    f"arn:aws:bedrock:{self.region}::foundation-model/anthropic.claude-haiku-4-5-20250818-v1:0",
                    # Cross-region inference profiles for higher throughput
                    f"arn:aws:bedrock:{self.region}:{self.account}:inference-profile/us.anthropic.claude-sonnet-4-20250514-v1:0",
                    f"arn:aws:bedrock:{self.region}:{self.account}:inference-profile/us.anthropic.claude-haiku-4-5-20250818-v1:0",
                ],
            )
        )

        # Lambda invocation for "Bring Your Own Inference" pattern
        # Allows evaluation job to invoke docint-search Lambda directly
        self.evaluation_role.add_to_policy(
            iam.PolicyStatement(
                sid="LambdaInvocationForCustomInference",
                actions=["lambda:InvokeFunction"],
                resources=[
                    lambda_stack.search_fn.function_arn,
                ],
            )
        )

        # CloudWatch Logs: Read agent runtime logs for retrieve-and-generate mode
        # (Required when evaluating end-to-end RAG with agent responses)
        self.evaluation_role.add_to_policy(
            iam.PolicyStatement(
                sid="CloudWatchLogsReadForAgentTraces",
                actions=[
                    "logs:GetLogEvents",
                    "logs:FilterLogEvents",
                ],
                resources=[
                    f"arn:aws:logs:{self.region}:{self.account}:log-group:/aws/bedrock-agentcore/runtimes/*",
                    f"arn:aws:logs:{self.region}:{self.account}:log-group:/aws/lambda/docint-search{env_suffix}:*",
                ],
            )
        )

        # CloudWatch Logs: Write evaluation execution logs
        self.evaluation_role.add_to_policy(
            iam.PolicyStatement(
                sid="CloudWatchLogsWriteForEvaluation",
                actions=[
                    "logs:CreateLogGroup",
                    "logs:CreateLogStream",
                    "logs:PutLogEvents",
                ],
                resources=[
                    f"arn:aws:logs:{self.region}:{self.account}:log-group:/aws/bedrock/model-evaluation-jobs/*",
                ],
            )
        )

        # ============================================
        # CloudWatch Alarms for Metric Thresholds
        # ============================================

        # SNS topic for evaluation alerts (optional)
        # Uncomment to enable email notifications on metric degradation
        # alert_topic = sns.Topic(
        #     self, "RagEvalAlertTopic",
        #     topic_name=f"docint-rag-eval-alerts{env_suffix}",
        #     display_name="RAG Evaluation Alerts",
        # )

        # Note: CloudWatch metrics for evaluation jobs are published by Bedrock
        # under namespace: AWS/Bedrock/ModelEvaluation
        # Metric dimensions: JobName, EvaluationType, MetricName

        # Example alarm for Context Relevance (create after first evaluation run)
        # Uncomment and customize after baseline metrics are established:
        #
        # context_relevance_alarm = cloudwatch.Alarm(
        #     self, "LowContextRelevanceAlarm",
        #     alarm_name=f"docint-low-context-relevance{env_suffix}",
        #     metric=cloudwatch.Metric(
        #         namespace="AWS/Bedrock/ModelEvaluation",
        #         metric_name="ContextRelevance",
        #         dimensions_map={
        #             "JobName": f"docint-search-quality-v1{env_suffix}",
        #             "EvaluationType": "RETRIEVE_ONLY",
        #         },
        #         statistic="Average",
        #         period=Duration.hours(24),
        #     ),
        #     threshold=0.80,  # Alert if Context Relevance < 0.80
        #     evaluation_periods=1,
        #     comparison_operator=cloudwatch.ComparisonOperator.LESS_THAN_THRESHOLD,
        #     treat_missing_data=cloudwatch.TreatMissingData.IGNORE,
        # )
        # context_relevance_alarm.add_alarm_action(cw_actions.SnsAction(alert_topic))

        # ============================================
        # Outputs
        # ============================================

        CfnOutput(
            self,
            "DatasetBucketName",
            value=self.dataset_bucket.bucket_name,
            description="S3 bucket for RAG evaluation datasets (JSONL format)",
            export_name=f"docint-rag-eval-dataset-bucket{env_suffix}",
        )

        CfnOutput(
            self,
            "ResultsBucketName",
            value=self.results_bucket.bucket_name,
            description="S3 bucket for RAG evaluation results",
            export_name=f"docint-rag-eval-results-bucket{env_suffix}",
        )

        CfnOutput(
            self,
            "EvaluationRoleArn",
            value=self.evaluation_role.role_arn,
            description="IAM role ARN for Bedrock RAG evaluation execution",
            export_name=f"docint-rag-eval-role-arn{env_suffix}",
        )

        CfnOutput(
            self,
            "SearchLambdaArn",
            value=lambda_stack.search_fn.function_arn,
            description="docint-search Lambda ARN for custom inference evaluation",
        )

        CfnOutput(
            self,
            "NextSteps",
            value="See docs/RAG-EVALUATION.md for dataset creation and job execution instructions",
            description="How to run RAG evaluations",
        )

        # Store references for cross-stack access
        self.dataset_bucket_name = self.dataset_bucket.bucket_name
        self.results_bucket_name = self.results_bucket.bucket_name
