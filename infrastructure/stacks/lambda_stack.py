from aws_cdk import (
    Stack,
    Duration,
    aws_lambda as _lambda,
    aws_iam as iam,
    aws_ec2 as ec2,
    aws_s3 as s3,
    aws_s3_notifications as s3n,
    aws_sqs as sqs,
)
from constructs import Construct
from stacks.database_stack import DatabaseStack


class LambdaStack(Stack):
    def _create_lambda_role(self, name: str, database: DatabaseStack) -> iam.Role:
        """Create a base Lambda role with VPC and database access.

        All Lambdas need:
        - CloudWatch Logs access (AWSLambdaBasicExecutionRole)
        - VPC networking (AWSLambdaVPCAccessExecutionRole)
        - Database secret read access
        """
        role = iam.Role(
            self, name,
            assumed_by=iam.ServicePrincipal("lambda.amazonaws.com"),
            managed_policies=[
                iam.ManagedPolicy.from_aws_managed_policy_name("service-role/AWSLambdaBasicExecutionRole"),
                iam.ManagedPolicy.from_aws_managed_policy_name("service-role/AWSLambdaVPCAccessExecutionRole"),
            ],
        )
        # All Lambdas need to read database credentials from Secrets Manager
        database.cluster.secret.grant_read(role)
        return role

    def __init__(self, scope: Construct, id: str, database: DatabaseStack, environment: str = "", **kwargs):
        super().__init__(scope, id, **kwargs)
        self.environment = environment
        # Helper for backward-compatible naming: only add suffix if environment is explicitly set
        self.env_suffix = f"-{environment}" if environment else ""

        # Security group for Lambdas
        lambda_sg = database.lambda_sg

        # Role 1: Query role (search + compare) - needs Bedrock for embeddings
        query_role = self._create_lambda_role("QueryRole", database)
        query_role.add_to_policy(iam.PolicyStatement(
            actions=["bedrock:InvokeModel"],
            resources=["arn:aws:bedrock:*::foundation-model/amazon.titan-embed-text-v2:0"],
        ))

        # Role 2: Metadata role - only needs database (no S3, no Bedrock)
        metadata_role = self._create_lambda_role("MetadataRole", database)

        # Role 3: Ingest role - needs S3 + Bedrock for document processing
        ingest_role = self._create_lambda_role("IngestRole", database)
        ingest_role.add_to_policy(iam.PolicyStatement(
            actions=["bedrock:InvokeModel"],
            resources=["arn:aws:bedrock:*::foundation-model/amazon.titan-embed-text-v2:0"],
        ))
        # S3 permissions granted via grant_read() below (line 154) - no manual policy needed

        # Pass secret ARN instead of plaintext credentials
        # Lambdas will resolve this at runtime using the AWS SDK
        secret = database.cluster.secret

        # Common settings for all Lambdas (role is specified per-Lambda)
        common_base = dict(
            runtime=_lambda.Runtime.PROVIDED_AL2023,
            architecture=_lambda.Architecture.ARM_64,
            handler="bootstrap",
            memory_size=512,
            timeout=Duration.seconds(30),
            environment={
                "DB_SECRET_ARN": secret.secret_arn,
                "DB_HOST": database.cluster.cluster_endpoint.hostname,
                "DB_PORT": "5432",
                "DB_NAME": "docint",
                "RUST_LOG": "info",
            },
            vpc=database.vpc,
            vpc_subnets=ec2.SubnetSelection(subnet_type=ec2.SubnetType.PRIVATE_ISOLATED),
            security_groups=[lambda_sg],
        )

        self.search_fn = _lambda.Function(
            self, "SearchFn",
            function_name=f"docint-search{self.env_suffix}",
            code=_lambda.Code.from_asset("../target/lambda/lambda-search"),
            role=query_role,  # Needs Bedrock for query embeddings
            tracing=_lambda.Tracing.ACTIVE,  # Enable X-Ray tracing
            **common_base,
        )

        self.metadata_fn = _lambda.Function(
            self, "MetadataFn",
            function_name=f"docint-metadata{self.env_suffix}",
            code=_lambda.Code.from_asset("../target/lambda/lambda-metadata"),
            role=metadata_role,  # DB access only, no S3 or Bedrock
            tracing=_lambda.Tracing.ACTIVE,  # Enable X-Ray tracing
            **common_base,
        )

        self.compare_fn = _lambda.Function(
            self, "CompareFn",
            function_name=f"docint-compare{self.env_suffix}",
            code=_lambda.Code.from_asset("../target/lambda/lambda-compare"),
            role=query_role,  # Needs Bedrock for query embeddings (shares with search)
            tracing=_lambda.Tracing.ACTIVE,  # Enable X-Ray tracing
            **common_base,
        )

        # Dead Letter Queue for failed ingest events
        ingest_dlq = sqs.Queue(
            self, "IngestDlq",
            queue_name=f"docint-ingest-dlq{self.env_suffix}",
            retention_period=Duration.days(14),  # Keep failed events for 2 weeks
        )

        # Ingest Lambda needs longer timeout for processing large documents
        self.ingest_fn = _lambda.Function(
            self, "IngestFn",
            function_name=f"docint-ingest{self.env_suffix}",
            code=_lambda.Code.from_asset("../target/lambda/lambda-ingest"),
            role=ingest_role,  # Needs S3 + Bedrock for document processing
            timeout=Duration.minutes(5),  # Override default 30s timeout
            dead_letter_queue=ingest_dlq,  # Capture failed invocations
            tracing=_lambda.Tracing.ACTIVE,  # Enable X-Ray tracing
            **{k: v for k, v in common_base.items() if k != "timeout"},
        )

        # S3 bucket for document ingestion with auto-trigger
        self.docs_bucket = s3.Bucket(
            self, "DocsBucket",
            bucket_name=f"docint-docs{self.env_suffix}-{self.account}",
            encryption=s3.BucketEncryption.S3_MANAGED,
            block_public_access=s3.BlockPublicAccess.BLOCK_ALL,
            enforce_ssl=True,
            lifecycle_rules=[
                s3.LifecycleRule(
                    id="DeleteOldDocuments",
                    enabled=True,
                    expiration=Duration.days(90),
                ),
                s3.LifecycleRule(
                    id="TransitionToIA",
                    enabled=True,
                    transitions=[
                        s3.Transition(
                            storage_class=s3.StorageClass.INFREQUENT_ACCESS,
                            transition_after=Duration.days(30),
                        )
                    ],
                ),
            ],
        )
        # Only ingest Lambda needs S3 access
        self.docs_bucket.grant_read(ingest_role)

        # Trigger ingest Lambda on supported text file uploads
        for suffix in [".txt", ".md", ".csv", ".json", ".html", ".xml", ".yaml", ".yml", ".log", ".rst"]:
            self.docs_bucket.add_event_notification(
                s3.EventType.OBJECT_CREATED,
                s3n.LambdaDestination(self.ingest_fn),
                s3.NotificationKeyFilter(suffix=suffix),
            )
