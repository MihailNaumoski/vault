#!/bin/bash
# Block reading of secret/sensitive files
# Called as a pre-hook on Read tool

INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty')

# Patterns to block
BLOCKED_PATTERNS=(
  '\.env$'
  '\.env\.'
  'credentials'
  '\.key$'
  '\.pem$'
  '\.secret'
  'secrets\.ya?ml'
  'service.account\.json'
)

for pattern in "${BLOCKED_PATTERNS[@]}"; do
  if echo "$FILE_PATH" | grep -qiE "$pattern"; then
    echo '{"decision":"block","reason":"🛡️ BLOCKED: Cannot read secret file '"$FILE_PATH"'. Ask the user for permission first."}' >&2
    # Hook protocol: exit 2 = block
    echo '{"decision":"block","reason":"🛡️ BLOCKED: Cannot read secret file '"$FILE_PATH"'. Ask the user for permission first."}'
    exit 2
  fi
done
