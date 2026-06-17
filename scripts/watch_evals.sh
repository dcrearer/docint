#!/bin/bash

LOG_GROUP="/aws/bedrock-agentcore/evaluations/results/evaluation_docint_cli_1781201794601-79s71JBb83"
LOG_STREAM="evaluation-results-evaluation_docint_cli_1781201794601-79s71JBb83"
CHECK_INTERVAL=${1:-30}  # seconds between checks

echo "Watching for new evaluations (checking every ${CHECK_INTERVAL}s)..."
echo "Press Ctrl+C to stop"
echo ""

# Track last seen timestamp
LAST_TIMESTAMP=$(date +%s)000

while true; do
    # Get events since last check
    NEW_EVENTS=$(aws logs get-log-events \
        --log-group-name "$LOG_GROUP" \
        --log-stream-name "$LOG_STREAM" \
        --start-time $LAST_TIMESTAMP \
        2>/dev/null | jq -r '.events[]')

    if [ -n "$NEW_EVENTS" ]; then
        # Parse and display new evaluations
        echo "$NEW_EVENTS" | jq -r '
            "\(.timestamp | tonumber / 1000 | strftime("%H:%M:%S")): " +
            (.message | fromjson | .attributes."gen_ai.evaluation.name") + " = " +
            (.message | fromjson | .attributes."gen_ai.evaluation.score.value" | tostring) + " (" +
            (.message | fromjson | .attributes."gen_ai.evaluation.score.label") + ")"
        ' 2>/dev/null

        # Update last seen timestamp
        LAST_TIMESTAMP=$(($(date +%s) * 1000))
    else
        echo -n "."
    fi

    sleep $CHECK_INTERVAL
done
