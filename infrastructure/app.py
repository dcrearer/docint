#!/usr/bin/env python3
import os
import aws_cdk as cdk
from stacks.database_stack import DatabaseStack
from stacks.lambda_stack import LambdaStack
from stacks.gateway_stack import GatewayStack
from stacks.agent_stack import AgentStack
from stacks.monitoring_stack import MonitoringStack
from stacks.auth_stack import AuthStack

app = cdk.App()

# Uses CDK_DEFAULT_ACCOUNT/CDK_DEFAULT_REGION from `aws configure` or env vars.
# No hardcoded account IDs.
env = cdk.Environment(
    account=os.environ.get("CDK_DEFAULT_ACCOUNT"),
    region=os.environ.get("CDK_DEFAULT_REGION", "us-east-1"),
)

AuthStack(app, "DocintAuthStack", env=env)
db_stack = DatabaseStack(app, "DocintDatabaseStack", env=env)
lambda_stack = LambdaStack(app, "DocintLambdaStack", database=db_stack, env=env)
gateway_stack = GatewayStack(app, "DocintGatewayStack", lambdas=lambda_stack, env=env)
AgentStack(app, "DocintAgentStack", gateway=gateway_stack, env=env)
MonitoringStack(app, "DocintMonitoringStack", lambdas=lambda_stack, env=env)

app.synth()
