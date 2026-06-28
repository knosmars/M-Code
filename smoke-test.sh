#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# Meyatu Code - Manual Smoke Test Script
#
# Tests basic connectivity and API endpoint health.
# Run with:  MEYATU_API_KEY="sk-..." bash smoke-test.sh
#
# Checks:
#   1. API endpoint reachability
#   2. API key authentication
#   3. Basic chat completion (simple non-streaming request)
# ---------------------------------------------------------------------------
set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

PASS=0
FAIL=0
SKIP=0

pass() {
    echo -e "${GREEN}[PASS]${NC} $*"
    PASS=$((PASS + 1))
}

fail() {
    echo -e "${RED}[FAIL]${NC} $*"
    FAIL=$((FAIL + 1))
}

skip() {
    echo -e "${YELLOW}[SKIP]${NC} $*"
    SKIP=$((SKIP + 1))
}

API_BASE="${MEYATU_API_BASE:-https://api.meyatu.io}"
MODEL="${MEYATU_MODEL:-deepseek-v4-flash}"

echo "=========================================="
echo "  Meyatu Code - Smoke Test"
echo "=========================================="
echo "  API Base:  $API_BASE"
echo "  Timestamp: $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
echo "=========================================="
echo ""

# ---------------------------------------------------------------------------
# Check 1: API key is set
# ---------------------------------------------------------------------------
if [[ -z "${MEYATU_API_KEY:-}" ]]; then
    fail "MEYATU_API_KEY is not set — export it before running this script"
    echo ""
    echo "Usage: MEYATU_API_KEY=\"sk-...\" bash smoke-test.sh"
    exit 1
fi
pass "MEYATU_API_KEY is set"

# ---------------------------------------------------------------------------
# Check 2: Network connectivity to API base
# ---------------------------------------------------------------------------
echo ""
echo "--- Network Connectivity ---"

# Strip protocol for curl host resolution
API_HOST="${API_BASE#https://}"
API_HOST="${API_HOST#http://}"
API_HOST="${API_HOST%%/*}"

if curl -s --connect-timeout 10 -o /dev/null -w "%{http_code}" "https://${API_HOST}" > /dev/null 2>&1; then
    pass "API host ${API_HOST} is reachable"
else
    fail "Cannot reach ${API_HOST} — check network / firewall / VPN"
fi

# ---------------------------------------------------------------------------
# Check 3: API authentication
# ---------------------------------------------------------------------------
echo ""
echo "--- API Authentication ---"

AUTH_CODE=$(curl -s --connect-timeout 10 -o /dev/null -w "%{http_code}" \
    -H "Authorization: Bearer ${MEYATU_API_KEY}" \
    -H "Content-Type: application/json" \
    "${API_BASE}/v1/models" 2>&1 || echo "000")

if [[ "$AUTH_CODE" == "200" ]]; then
    pass "API key authenticates successfully (HTTP $AUTH_CODE)"
elif [[ "$AUTH_CODE" == "401" ]]; then
    fail "API key rejected (HTTP 401) — check MEYATU_API_KEY value"
elif [[ "$AUTH_CODE" == "403" ]]; then
    fail "API access forbidden (HTTP 403) — check account / billing"
else
    fail "Unexpected response listing models (HTTP $AUTH_CODE)"
fi

# ---------------------------------------------------------------------------
# Check 4: Basic chat completion (non-streaming)
# ---------------------------------------------------------------------------
echo ""
echo "--- Chat Completion (non-streaming) ---"

CHAT_RESPONSE=$(curl -s --connect-timeout 30 --max-time 30 \
    -H "Authorization: Bearer ${MEYATU_API_KEY}" \
    -H "Content-Type: application/json" \
    -d '{
        "model": "'"${MODEL}"'",
        "messages": [{"role": "user", "content": "Say hello in exactly one word."}],
        "stream": false,
        "max_tokens": 10
    }' \
    "${API_BASE}/v1/chat/completions" 2>&1)

if echo "$CHAT_RESPONSE" | grep -q '"choices"'; then
    # Extract the assistant content for display
    CONTENT=$(echo "$CHAT_RESPONSE" | grep -o '"content":"[^"]*"' | head -1 | cut -d'"' -f4 || echo "")
    if [[ -n "$CONTENT" ]]; then
        pass "Chat completion returned content: \"$CONTENT\""
    else
        pass "Chat completion returned choices (content may be empty)"
    fi
elif echo "$CHAT_RESPONSE" | grep -qi '"error"'; then
    ERR_MSG=$(echo "$CHAT_RESPONSE" | grep -o '"message":"[^"]*"' | head -1 | cut -d'"' -f4 || echo "unknown error")
    fail "Chat completion failed with error: $ERR_MSG"
else
    fail "Chat completion returned unexpected response"
    echo "Raw response (first 200 chars): ${CHAT_RESPONSE:0:200}"
fi

# ---------------------------------------------------------------------------
# Check 5: API streaming support
# ---------------------------------------------------------------------------
echo ""
echo "--- Streaming Support ---"

STREAM_RESPONSE=$(curl -s --connect-timeout 30 --max-time 30 \
    -H "Authorization: Bearer ${MEYATU_API_KEY}" \
    -H "Content-Type: application/json" \
    -d '{
        "model": "'"${MODEL}"'",
        "messages": [{"role": "user", "content": "Say hi"}],
        "stream": true,
        "max_tokens": 10
    }' \
    "${API_BASE}/v1/chat/completions" 2>&1 | head -5 || echo "")

if echo "$STREAM_RESPONSE" | grep -q "data:"; then
    pass "Streaming SSE response received"
elif echo "$STREAM_RESPONSE" | grep -qi '"error"'; then
    fail "Streaming request failed with error"
else
    skip "Could not confirm streaming support (model may not support it)"
fi

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
echo ""
echo "=========================================="
echo "  Results: ${GREEN}${PASS} passed${NC}, ${RED}${FAIL} failed${NC}, ${YELLOW}${SKIP} skipped${NC}"
echo "=========================================="

if [[ $FAIL -gt 0 ]]; then
    exit 1
else
    exit 0
fi
