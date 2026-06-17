#!/bin/bash
set -e

# Configuration
TOTAL_PROMPTS=${1:-100}
PROMPTS_PER_SESSION=${2:-5}
DELAY_BETWEEN_SESSIONS=${3:-0}  # seconds, set to 6 for evaluation generation

# Color output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Prompt variations for realistic testing
PROMPTS=(
    "list my documents"
    "search for rust lifetime"
    "what documents are about DNS"
    "show me documents about distributed systems"
    "search for AWS"
    "what documents mention observability"
    "list documents about SRE"
    "search for python"
    "what documents are about career planning"
    "show me networking documents"
    "search for route 53"
    "what documents mention EDNS"
    "list documents about memory management"
    "search for concurrency"
    "what documents are about GPU instances"
)

# Calculate sessions needed
NUM_SESSIONS=$(( (TOTAL_PROMPTS + PROMPTS_PER_SESSION - 1) / PROMPTS_PER_SESSION ))

echo -e "${GREEN}=== DocInt Load Test ===${NC}"
echo "Total prompts: $TOTAL_PROMPTS"
echo "Prompts per session: $PROMPTS_PER_SESSION"
echo "Sessions to run: $NUM_SESSIONS"
echo "Delay between sessions: ${DELAY_BETWEEN_SESSIONS}s"
echo ""

START_TIME=$(date +%s)
PROMPT_COUNT=0

for session in $(seq 1 $NUM_SESSIONS); do
    echo -e "${BLUE}[Session $session/$NUM_SESSIONS]${NC}"

    # Calculate how many prompts for this session
    REMAINING=$(( TOTAL_PROMPTS - PROMPT_COUNT ))
    SESSION_PROMPTS=$(( REMAINING < PROMPTS_PER_SESSION ? REMAINING : PROMPTS_PER_SESSION ))

    # Build prompt list for this session
    PROMPT_LIST=""
    for i in $(seq 1 $SESSION_PROMPTS); do
        # Cycle through prompt variations
        PROMPT_INDEX=$(( (PROMPT_COUNT + i - 1) % ${#PROMPTS[@]} ))
        PROMPT_LIST="${PROMPT_LIST}${PROMPTS[$PROMPT_INDEX]}\n"
    done
    PROMPT_LIST="${PROMPT_LIST}quit"

    # Send prompts
    echo -e "  Sending $SESSION_PROMPTS prompts..."
    if echo -e "$PROMPT_LIST" | docint-cli > /dev/null 2>&1; then
        echo -e "  ${GREEN}✓${NC} Session complete"
    else
        echo -e "  ${YELLOW}⚠${NC} Session had errors (continuing...)"
    fi

    PROMPT_COUNT=$(( PROMPT_COUNT + SESSION_PROMPTS ))
    echo -e "  Progress: $PROMPT_COUNT/$TOTAL_PROMPTS prompts"

    # Delay between sessions if specified
    if [ $session -lt $NUM_SESSIONS ] && [ $DELAY_BETWEEN_SESSIONS -gt 0 ]; then
        echo -e "  Waiting ${DELAY_BETWEEN_SESSIONS}s before next session..."
        sleep $DELAY_BETWEEN_SESSIONS
    fi
    echo ""
done

END_TIME=$(date +%s)
DURATION=$(( END_TIME - START_TIME ))

echo -e "${GREEN}=== Test Complete ===${NC}"
echo "Total prompts sent: $PROMPT_COUNT"
echo "Total sessions: $NUM_SESSIONS"
echo "Duration: ${DURATION}s"
echo "Average: $(( DURATION / NUM_SESSIONS ))s per session"
echo ""
echo -e "${BLUE}Check logs:${NC}"
echo "  Agent logs:     aws logs tail /aws/bedrock-agentcore/runtimes/docint_agent-lsc56PDJsX-docint_agent_endpoint --since ${DURATION}s"
echo "  Evaluation logs: aws logs tail /aws/bedrock-agentcore/evaluations/results/evaluation_docint_cli_1781201794601-79s71JBb83 --since ${DURATION}s"
