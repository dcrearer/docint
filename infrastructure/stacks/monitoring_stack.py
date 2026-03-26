from aws_cdk import (
    Stack,
    Duration,
    aws_cloudwatch as cw,
    aws_sns as sns,
    aws_cloudwatch_actions as cw_actions,
)
from constructs import Construct
from stacks.lambda_stack import LambdaStack


class MonitoringStack(Stack):
    def __init__(self, scope: Construct, id: str, lambdas: LambdaStack, **kwargs):
        super().__init__(scope, id, **kwargs)

        # SNS topic for alarm notifications
        self.alarm_topic = sns.Topic(self, "AlarmTopic", topic_name="docint-alarms")
        alarm_action = cw_actions.SnsAction(self.alarm_topic)

        functions = {
            "Search": lambdas.search_fn,
            "Metadata": lambdas.metadata_fn,
            "Compare": lambdas.compare_fn,
            "Ingest": lambdas.ingest_fn,
        }

        # --- Alarms per Lambda ---
        for name, fn in functions.items():
            # Error rate alarm: fires if >3 errors in 5 minutes
            error_alarm = fn.metric_errors(period=Duration.minutes(5)).create_alarm(
                self, f"{name}Errors",
                alarm_name=f"docint-{name.lower()}-errors",
                threshold=3,
                evaluation_periods=1,
                treat_missing_data=cw.TreatMissingData.NOT_BREACHING,
            )
            error_alarm.add_alarm_action(alarm_action)

            # Duration alarm: fires if p99 latency > 10s for 2 consecutive periods
            duration_alarm = fn.metric_duration(
                period=Duration.minutes(5),
                statistic="p99",
            ).create_alarm(
                self, f"{name}Duration",
                alarm_name=f"docint-{name.lower()}-slow",
                threshold=10_000,  # 10 seconds in ms
                evaluation_periods=2,
                treat_missing_data=cw.TreatMissingData.NOT_BREACHING,
            )
            duration_alarm.add_alarm_action(alarm_action)

        # --- Dashboard ---
        dashboard = cw.Dashboard(self, "Dashboard", dashboard_name="docint")

        # Row 1: Invocations and errors side by side
        dashboard.add_widgets(
            cw.GraphWidget(
                title="Invocations",
                width=12,
                left=[fn.metric_invocations(period=Duration.minutes(1)) for fn in functions.values()],
            ),
            cw.GraphWidget(
                title="Errors",
                width=12,
                left=[fn.metric_errors(period=Duration.minutes(1)) for fn in functions.values()],
            ),
        )

        # Row 2: Duration p50/p99 and throttles
        dashboard.add_widgets(
            cw.GraphWidget(
                title="Duration (p50)",
                width=12,
                left=[fn.metric_duration(period=Duration.minutes(1), statistic="p50") for fn in functions.values()],
            ),
            cw.GraphWidget(
                title="Duration (p99)",
                width=12,
                left=[fn.metric_duration(period=Duration.minutes(1), statistic="p99") for fn in functions.values()],
            ),
        )

        # Row 3: Concurrent executions and throttles
        dashboard.add_widgets(
            cw.GraphWidget(
                title="Concurrent Executions",
                width=12,
                left=[fn.metric(metric_name="ConcurrentExecutions", period=Duration.minutes(1), statistic="Maximum") for fn in functions.values()],
            ),
            cw.GraphWidget(
                title="Throttles",
                width=12,
                left=[fn.metric_throttles(period=Duration.minutes(1)) for fn in functions.values()],
            ),
        )

        # Row 4: Bedrock metrics
        bedrock_ns = "AWS/Bedrock"
        dashboard.add_widgets(
            cw.GraphWidget(
                title="Bedrock Invocations",
                width=12,
                left=[cw.Metric(
                    namespace=bedrock_ns,
                    metric_name="Invocations",
                    dimensions_map={"ModelId": "amazon.titan-embed-text-v2:0"},
                    period=Duration.minutes(1),
                )],
            ),
            cw.GraphWidget(
                title="Bedrock Latency",
                width=12,
                left=[cw.Metric(
                    namespace=bedrock_ns,
                    metric_name="InvocationLatency",
                    dimensions_map={"ModelId": "amazon.titan-embed-text-v2:0"},
                    period=Duration.minutes(1),
                    statistic="p99",
                )],
            ),
        )
