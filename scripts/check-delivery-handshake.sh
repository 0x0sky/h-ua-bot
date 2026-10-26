#!/usr/bin/env bash
# © 2026 aiaiaiai · aiaiaiai.org
# SPDX-License-Identifier: MIT
#
# Read-only production delivery handshake. Invalid JSON cannot reach Telegram.
# Run with PRISM_BOT_DELIVERY_SECRET taken from the Hub's configured runtime.
set -Eeuo pipefail
umask 077

: "${PRISM_BOT_DELIVERY_SECRET:?Hub delivery secret is required}"
if (( ${#PRISM_BOT_DELIVERY_SECRET} < 32 )) ||
  [[ "$PRISM_BOT_DELIVERY_SECRET" == *$'\n'* || "$PRISM_BOT_DELIVERY_SECRET" == *$'\r'* ]]; then
  echo "Invalid delivery secret: expected a one-line value of at least 32 characters" >&2
  exit 1
fi

command -v curl >/dev/null || { echo "curl is required" >&2; exit 1; }
command -v jq >/dev/null || { echo "jq is required" >&2; exit 1; }

origin="https://api.aiaiaiai.org"
temp="$(mktemp -d)"
trap 'rm -rf "$temp"' EXIT
printf 'x-prism-bot-delivery-secret: %s\n' "$PRISM_BOT_DELIVERY_SECRET" > "$temp/header"

check_health() {
  local path="$1" service="$2"
  curl --silent --show-error --fail --proto '=https' --max-time 15 \
    --output "$temp/health.json" "$origin$path"
  jq -e --arg service "$service" '.status == "ok" and (.service == $service or .service == null)' \
    "$temp/health.json" >/dev/null || {
    echo "Invalid health response from $service" >&2
    exit 1
  }
  echo "$service HTTP health: ok"
}

check_health "/prism-hub/healthz" "prism-hub"
check_health "/hua-bot/health" "h-ua-bot"

# This body deliberately omits chat_id, text and idempotency_key.
# 400/invalid_request means the shared secret passed authentication.
# 401 means the Hub's secret does not match the bot's runtime secret.
status="$(curl --silent --show-error --proto '=https' --max-time 15 \
  --header "content-type: application/json" \
  --header "@$temp/header" \
  --data-binary '{}' \
  --output "$temp/delivery.json" \
  --write-out '%{http_code}' \
  "$origin/hua-bot/api/v1/delivery")"
if [[ "$status" != 400 ]] ||
  ! jq -e '.status == "error" and .error.code == "invalid_request"' "$temp/delivery.json" >/dev/null; then
  echo "Delivery handshake failed (HTTP $status): check Hub/bot secret parity and edge routing" >&2
  exit 1
fi

echo "Hub-to-bot delivery authentication: accepted; invalid payload rejected (no Telegram message sent)"
echo "NOT VERIFIED: hub principal, subscriptions, running scheduler, provider API, or real Telegram delivery"
