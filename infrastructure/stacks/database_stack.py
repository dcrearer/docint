from aws_cdk import (
    Stack,
    RemovalPolicy,
    CfnOutput,
    aws_ec2 as ec2,
    aws_rds as rds,
)
from constructs import Construct


class DatabaseStack(Stack):
    def __init__(self, scope: Construct, id: str, **kwargs):
        super().__init__(scope, id, **kwargs)

        # VPC — no NAT gateway, use VPC endpoints instead
        self.vpc = ec2.Vpc(
            self, "Vpc",
            max_azs=2,
            nat_gateways=0,
            subnet_configuration=[
                ec2.SubnetConfiguration(name="Private", subnet_type=ec2.SubnetType.PRIVATE_ISOLATED, cidr_mask=24),
                ec2.SubnetConfiguration(name="Public", subnet_type=ec2.SubnetType.PUBLIC, cidr_mask=24),
            ],
        )

        # VPC endpoints for AWS services (replaces NAT gateway)
        self.vpc.add_gateway_endpoint("S3Endpoint", service=ec2.GatewayVpcEndpointAwsService.S3)

        for svc_name, svc in [
            ("Bedrock", ec2.InterfaceVpcEndpointAwsService.BEDROCK_RUNTIME),
            ("SecretsManager", ec2.InterfaceVpcEndpointAwsService.SECRETS_MANAGER),
            ("STS", ec2.InterfaceVpcEndpointAwsService.STS),
        ]:
            self.vpc.add_interface_endpoint(
                f"{svc_name}Endpoint",
                service=svc,
                subnets=ec2.SubnetSelection(subnet_type=ec2.SubnetType.PRIVATE_ISOLATED),
            )

        # Security groups
        self.db_sg = ec2.SecurityGroup(self, "DbSg", vpc=self.vpc, description="PostgreSQL access")
        self.lambda_sg = ec2.SecurityGroup(self, "LambdaSg", vpc=self.vpc, description="Lambda functions")
        self.db_sg.add_ingress_rule(self.lambda_sg, ec2.Port.tcp(5432), "Lambda to Postgres")

        # Aurora Serverless v2
        self.cluster = rds.DatabaseCluster(
            self, "Cluster",
            engine=rds.DatabaseClusterEngine.aurora_postgres(
                version=rds.AuroraPostgresEngineVersion.VER_16_6,
            ),
            default_database_name="docint",
            credentials=rds.Credentials.from_generated_secret("docint"),
            writer=rds.ClusterInstance.serverless_v2("Writer", scale_with_writer=True),
            vpc=self.vpc,
            vpc_subnets=ec2.SubnetSelection(subnet_type=ec2.SubnetType.PRIVATE_ISOLATED),
            security_groups=[self.db_sg],
            removal_policy=RemovalPolicy.SNAPSHOT,
            enable_data_api=True,
        )

        CfnOutput(self, "ClusterEndpoint", value=self.cluster.cluster_endpoint.hostname)
        CfnOutput(self, "ClusterArn", value=self.cluster.cluster_arn)
        CfnOutput(self, "SecretArn", value=self.cluster.secret.secret_arn)
        CfnOutput(self, "VpcId", value=self.vpc.vpc_id)
