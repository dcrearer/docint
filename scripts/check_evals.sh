#!/bin/bash

LOG_GROUP="/aws/bedrock-agentcore/evaluations/results/evaluation_docint_cli_1781201794601-79s71JBb83"
LOG_STREAM="evaluation-results-evaluation_docint_cli_1781201794601-79s71JBb83"
SINCE=$(($(date +%s) - 180))000

echo "=== Evaluation Monitor ==="
echo "Time: $(date '+%Y-%m-%d %H:%M:%S')"
echo ""

aws logs get-log-events \
  --log-group-name "$LOG_GROUP" \
  --log-stream-name "$LOG_STREAM" \
  --start-time $SINCE \
  --limit 30 2>&1 | jq -r '
  if (.events | length) > 0 then
    "📊 NEW EVALUATIONS FOUND\n" +
    "─────────────────────────────────────\n" +
    (.events | sort_by(.timestamp) | .[] |
      (.message | fromjson) as $m |
      "\n⏱️  Time: " + (.timestamp / 1000 | strftime("%H:%M:%S")) +
      "\n📝 Evaluator: " + ($m.attributes."gen_ai.evaluation.name") +
      "\n   Score: " + ($m.attributes."gen_ai.evaluation.score.value" | tostring) +
      " (" + ($m.attributes."gen_ai.evaluation.score.label") + ")" +
      "\n   Session: " + (($m.attributes."session.id" // "N/A") | .[0:20]) +
      "\n"
    ) +
    "\n─────────────────────────────────────" +
    "\n✅ Total: " + (.events | length | tostring) + " evaluations"
  else
    "⏳ No new evaluations in last 3 minutes\n" +
    "   Waiting for sessions to complete...\n" +
    "   (Need 5min timeout + 2-5min processing)\n" +
    "\n   Current time: " + (now | strftime("%H:%M:%S")) +
    "\n   Load test started: ~01:26\n" +
    "   Expected evals: ~01:34-01:40"
  end
'
