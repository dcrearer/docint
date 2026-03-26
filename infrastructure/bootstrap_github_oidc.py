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
                    },
                    "StringLike": {
                        # CHANGE THIS to your repo: "your-username/docint"
                        "token.actions.githubusercontent.com:sub": "repo:dcrearer/docint:*",
                    },
                },
                assume_role_action="sts:AssumeRoleWithWebIdentity",
            ),
        )

        # Permissions the deploy role needs.
        # CloudFormation + CDK need broad permissions to create resources.
        role.add_managed_policy(
            iam.ManagedPolicy.from_aws_managed_policy_name("AdministratorAccess")
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
