#!/usr/bin/env python3
"""
One-time setup for CloudWatch Transaction Search.
Required for AgentCore Evaluations to index X-Ray traces.

Usage:
    python3 setup_transaction_search.py --region us-east-1
"""
import argparse
import boto3
import json
import sys


def enable_transaction_search(region: str):
    """Enable CloudWatch Transaction Search for X-Ray traces."""
    xray = boto3.client("xray", region_name=region)
    logs = boto3.client("logs", region_name=region)
    sts = boto3.client("sts", region_name=region)

    account_id = sts.get_caller_identity()["Account"]

    print(f"Configuring Transaction Search in {region} for account {account_id}...")
    print()

    # 1. Create CloudWatch Logs resource policy for X-Ray
    print("→ Creating CloudWatch Logs resource policy for X-Ray delivery...")
    policy_doc = {
        "Version": "2012-10-17",
        "Statement": [{
            "Sid": "AWSLogDeliveryWrite",
            "Effect": "Allow",
            "Principal": {"Service": "delivery.logs.amazonaws.com"},
            "Action": ["logs:CreateLogStream", "logs:PutLogEvents"],
            "Resource": f"arn:aws:logs:{region}:{account_id}:log-group:aws/spans:*",
            "Condition": {
                "StringEquals": {"aws:SourceAccount": account_id}
            }
        }]
    }

    try:
        logs.put_resource_policy(
            policyName="AWSLogDeliveryXRayPolicy",
            policyDocument=json.dumps(policy_doc)
        )
        print("  ✓ Resource policy created")
    except logs.exceptions.InvalidParameterException as e:
        if "already exists" in str(e).lower():
            print("  ✓ Resource policy already exists")
        else:
            print(f"  ✗ Error: {e}")
            return False
    except Exception as e:
        print(f"  ✗ Error: {e}")
        return False

    # 2. Update X-Ray trace segment destination to CloudWatch Logs
    print("→ Updating X-Ray trace segment destination to CloudWatch Logs...")
    try:
        xray.update_trace_segment_destination(destination="CloudWatchLogs")
        print("  ✓ Trace destination set to CloudWatch Logs")
    except Exception as e:
        error_msg = str(e)
        if "already" in error_msg.lower():
            print("  ✓ Already configured")
        else:
            print(f"  ⚠ Warning: {e}")

    # 3. Update sampling percentage (100% for comprehensive traces)
    print("→ Configuring X-Ray sampling (100%)...")
    try:
        xray.update_indexing_rule(
            rule={
                "Sampled": True,
                "SamplingPercentage": 100.0
            }
        )
        print("  ✓ Sampling configured at 100%")
    except Exception as e:
        error_msg = str(e)
        if "already" in error_msg.lower() or "no change" in error_msg.lower():
            print("  ✓ Already configured")
        else:
            print(f"  ⚠ Warning: {e}")

    print()
    print("✅ Transaction Search setup complete!")
    print()
    print("Traces will now be indexed in CloudWatch Logs under 'aws/spans' log group.")
    print("This log group will be created automatically when traces start flowing.")
    print()
    print("Next steps:")
    print("  1. Deploy the instrumented agent (git push)")
    print("  2. Invoke the agent to generate traces")
    print("  3. Create evaluation config in AWS Console")

    return True


if __name__ == "__main__":
    parser = argparse.ArgumentParser(
        description="Enable CloudWatch Transaction Search for AgentCore Evaluations",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Example:
    python3 setup_transaction_search.py --region us-east-1

This script configures:
  • CloudWatch Logs resource policy for X-Ray
  • X-Ray trace destination → CloudWatch Logs
  • Sampling percentage (100% for comprehensive coverage)

Safe to re-run if already configured.
        """
    )
    parser.add_argument(
        "--region",
        default="us-east-1",
        help="AWS region (default: us-east-1)"
    )
    args = parser.parse_args()

    success = enable_transaction_search(args.region)
    sys.exit(0 if success else 1)
