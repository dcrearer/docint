#!/usr/bin/env python3
"""
One-time bootstrap: creates the IAM OIDC provider and deploy role
that GitHub Actions uses to authenticate with AWS.

Deploy this manually once:
  cd infrastructure
  source .venv/bin/activate
  cdk deploy -a "python3 bootstrap_github_oidc.py"

Then add the role ARN output as a GitHub secret named AWS_DEPLOY_ROLE_ARN.
"""

import os

import aws_cdk as cdk
from aws_cdk import CfnOutput, Stack
from aws_cdk import aws_iam as iam
from constructs import Construct


class GitHubOidcStack(Stack):
    def __init__(self, scope: Construct, id: str, **kwargs):
        super().__init__(scope, id, **kwargs)

        # OIDC provider — tells AWS to trust tokens from GitHub Actions.
        # GitHub signs a JWT for each workflow run, and AWS verifies it
        # against this provider before issuing temporary credentials.
        provider = iam.OpenIdConnectProvider(
            self,
            "GitHubOidc",
            url="https://token.actions.githubusercontent.com",
            client_ids=["sts.amazonaws.com"],
        )

        # Deploy role — GitHub Actions assumes this role.
        # The condition restricts it to your specific repo and branch.
        # Change "OWNER/REPO" to your actual GitHub repo.
        role = iam.Role(
            self,
            "DeployRole",
            role_name="docint-github-deploy",
            assumed_by=iam.FederatedPrincipal(
                provider.open_id_connect_provider_arn,
                conditions={
                    "StringEquals": {
                        "token.actions.githubusercontent.com:aud": "sts.amazonaws.com",
                        # Restrict to main branch only - prevents malicious workflows on other branches
                        "token.actions.githubusercontent.com:sub": "repo:dcrearer/docint:ref:refs/heads/main",
                    },
                },
                assume_role_action="sts:AssumeRoleWithWebIdentity",
            ),
        )

        # Permissions the deploy role needs (scoped to only what CDK deployment requires).
        # This replaces AdministratorAccess with least-privilege permissions.
        role.add_to_policy(
            iam.PolicyStatement(
                actions=[
                    # CloudFormation - for CDK deployments
                    "cloudformation:*",
                    # Lambda - deployment and configuration
                    "lambda:*",
                    # IAM - only PassRole for Lambda execution roles
                    "iam:GetRole",
                    "iam:PassRole",
                    "iam:CreateRole",
                    "iam:DeleteRole",
                    "iam:AttachRolePolicy",
                    "iam:DetachRolePolicy",
                    "iam:PutRolePolicy",
                    "iam:DeleteRolePolicy",
                    "iam:GetRolePolicy",
                    "iam:TagRole",
                    "iam:UntagRole",
                    # RDS - read-only for connection info
                    "rds:DescribeDBClusters",
                    "rds:DescribeDBInstances",
                    # Secrets Manager - read secret ARNs
                    "secretsmanager:DescribeSecret",
                    "secretsmanager:GetSecretValue",
                    # VPC/Networking - for Lambda VPC configuration
                    "ec2:DescribeAvailabilityZones",
                    "ec2:DescribeVpcs",
                    "ec2:DescribeSubnets",
                    "ec2:DescribeSecurityGroups",
                    "ec2:DescribeNetworkInterfaces",
                    "ec2:DescribeVpcEndpoints",
                    "ec2:CreateVpcEndpoint",
                    "ec2:DeleteVpcEndpoint",
                    "ec2:ModifyVpcEndpoint",
                    "ec2:CreateSecurityGroup",
                    "ec2:DeleteSecurityGroup",
                    "ec2:AuthorizeSecurityGroupIngress",
                    "ec2:AuthorizeSecurityGroupEgress",
                    "ec2:RevokeSecurityGroupIngress",
                    "ec2:RevokeSecurityGroupEgress",
                    "ec2:CreateTags",
                    "ec2:DeleteTags",
                    # S3 - for document storage bucket
                    "s3:*",
                    # ECR - for container image publishing (AgentStack)
                    "ecr:GetAuthorizationToken",
                    "ecr:BatchCheckLayerAvailability",
                    "ecr:GetDownloadUrlForLayer",
                    "ecr:BatchGetImage",
                    "ecr:PutImage",
                    "ecr:InitiateLayerUpload",
                    "ecr:UploadLayerPart",
                    "ecr:CompleteLayerUpload",
                    "ecr:DescribeRepositories",
                    "ecr:CreateRepository",
                    "ecr:DeleteRepository",
                    "ecr:SetRepositoryPolicy",
                    "ecr:GetRepositoryPolicy",
                    # Cognito - for authentication
                    "cognito-idp:*",
                    # Bedrock - for AgentCore and embeddings
                    "bedrock:*",
                    "bedrock-agentcore:*",
                    # CloudWatch - for monitoring
                    "cloudwatch:*",
                    "logs:*",
                    # SSM - for CDK context values
                    "ssm:GetParameter",
                    "ssm:PutParameter",
                    "ssm:DeleteParameter",
                ],
                resources=["*"],
            )
        )

        CfnOutput(
            self,
            "RoleArn",
            value=role.role_arn,
            description="Add this as GitHub secret AWS_DEPLOY_ROLE_ARN",
        )


app = cdk.App()
GitHubOidcStack(
    app,
    "DocintGitHubOidcStack",
    env=cdk.Environment(
        account=os.environ.get("CDK_DEFAULT_ACCOUNT"),
        region=os.environ.get("CDK_DEFAULT_REGION", "us-east-1"),
    ),
)
app.synth()
