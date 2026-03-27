from aws_cdk import (
    Stack,
    Duration,
    aws_lambda as _lambda,
    aws_iam as iam,
    aws_ec2 as ec2,
    aws_s3 as s3,
    aws_s3_notifications as s3n,
)
from constructs import Construct
from stacks.database_stack import DatabaseStack


class LambdaStack(Stack):
    def __init__(self, scope: Construct, id: str, database: DatabaseStack, **kwargs):
        super().__init__(scope, id, **kwargs)

        # Security group for Lambdas
        lambda_sg = database.lambda_sg

        # Shared role
        role = iam.Role(
            self, "LambdaRole",
            assumed_by=iam.ServicePrincipal("lambda.amazonaws.com"),
            managed_policies=[
                iam.ManagedPolicy.from_aws_managed_policy_name("service-role/AWSLambdaBasicExecutionRole"),
                iam.ManagedPolicy.from_aws_managed_policy_name("service-role/AWSLambdaVPCAccessExecutionRole"),
            ],
        )
        role.add_to_policy(iam.PolicyStatement(
            actions=["bedrock:InvokeModel"],
            resources=["arn:aws:bedrock:*::foundation-model/amazon.titan-embed-text-v2:0"],
        ))
        role.add_to_policy(iam.PolicyStatement(
            actions=["s3:GetObject"],
            resources=["arn:aws:s3:::docint-*/*"],
        ))
        database.cluster.secret.grant_read(role)

        # Build DATABASE_URL from secret
        secret = database.cluster.secret
        db_url = f"postgres://{{{{resolve:secretsmanager:{secret.secret_arn}:SecretString:username}}}}:{{{{resolve:secretsmanager:{secret.secret_arn}:SecretString:password}}}}@{database.cluster.cluster_endpoint.hostname}:5432/docint"

        common = dict(
            runtime=_lambda.Runtime.PROVIDED_AL2023,
            architecture=_lambda.Architecture.ARM_64,
            handler="bootstrap",
            role=role,
            memory_size=512,
            timeout=Duration.seconds(30),
            environment={"DATABASE_URL": db_url},
            vpc=database.vpc,
            vpc_subnets=ec2.SubnetSelection(subnet_type=ec2.SubnetType.PRIVATE_ISOLATED),
            security_groups=[lambda_sg],
            tracing=_lambda.Tracing.ACTIVE,
        )

        self.search_fn = _lambda.Function(
            self, "SearchFn",
            function_name="docint-search",
            code=_lambda.Code.from_asset("../target/lambda/lambda-search"),
            **common,
        )

        self.metadata_fn = _lambda.Function(
            self, "MetadataFn",
            function_name="docint-metadata",
            code=_lambda.Code.from_asset("../target/lambda/lambda-metadata"),
            **common,
        )

        self.compare_fn = _lambda.Function(
            self, "CompareFn",
            function_name="docint-compare",
            code=_lambda.Code.from_asset("../target/lambda/lambda-compare"),
            **common,
        )

        self.ingest_fn = _lambda.Function(
            self, "IngestFn",
            function_name="docint-ingest",
            code=_lambda.Code.from_asset("../target/lambda/lambda-ingest"),
            timeout=Duration.minutes(5),
            **{k: v for k, v in common.items() if k != "timeout"},
        )

        # S3 bucket for document ingestion with auto-trigger
        self.docs_bucket = s3.Bucket(
            self, "DocsBucket",
            bucket_name=f"docint-docs-{self.account}",
            encryption=s3.BucketEncryption.S3_MANAGED,
        )
        self.docs_bucket.grant_read(role)

        # Trigger ingest Lambda on supported text file uploads
        for suffix in [".txt", ".md", ".csv", ".json", ".html", ".xml", ".yaml", ".yml", ".log", ".rst"]:
            self.docs_bucket.add_event_notification(
                s3.EventType.OBJECT_CREATED,
                s3n.LambdaDestination(self.ingest_fn),
                s3.NotificationKeyFilter(suffix=suffix),
            )
