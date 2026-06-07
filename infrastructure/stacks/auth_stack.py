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

        # Self sign-up disabled to prevent unauthorized access and cost abuse.
        # New users must be created by administrators via AWS Console or AdminCreateUser API.
        self.user_pool = cognito.UserPool(
            self, "UserPool",
            user_pool_name="docint-users",
            self_sign_up_enabled=False,
            sign_in_aliases=cognito.SignInAliases(username=True),
            password_policy=cognito.PasswordPolicy(
                min_length=8,
                require_lowercase=True,
                require_digits=True,
                require_uppercase=False,
                require_symbols=False,
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
