from aws_cdk import (
    Stack,
    CfnOutput,
    aws_cognito as cognito,
    aws_lambda as _lambda,
)
from constructs import Construct


class AuthStack(Stack):
    def __init__(self, scope: Construct, id: str, **kwargs):
        super().__init__(scope, id, **kwargs)

        # Pre-sign-up trigger: auto-confirm users (no email verification)
        auto_confirm_fn = _lambda.Function(
            self, "AutoConfirmFn",
            function_name="docint-auto-confirm",
            runtime=_lambda.Runtime.PYTHON_3_13,
            handler="index.handler",
            code=_lambda.Code.from_inline(
                "def handler(event, context):\n"
                "    event['response']['autoConfirmUser'] = True\n"
                "    return event\n"
            ),
        )

        self.user_pool = cognito.UserPool(
            self, "UserPool",
            user_pool_name="docint-users",
            self_sign_up_enabled=True,
            sign_in_aliases=cognito.SignInAliases(username=True),
            password_policy=cognito.PasswordPolicy(
                min_length=8,
                require_lowercase=True,
                require_digits=True,
                require_uppercase=False,
                require_symbols=False,
            ),
            lambda_triggers=cognito.UserPoolTriggers(
                pre_sign_up=auto_confirm_fn,
            ),
        )

        self.app_client = self.user_pool.add_client(
            "CliClient",
            user_pool_client_name="docint-cli",
            auth_flows=cognito.AuthFlow(
                user_password=True,
            ),
            generate_secret=False,
        )

        CfnOutput(self, "UserPoolId", value=self.user_pool.user_pool_id)
        CfnOutput(self, "ClientId", value=self.app_client.user_pool_client_id)
