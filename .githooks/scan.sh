#!/usr/bin/env bash
# Leak scanner — runs on pre-commit, pre-push, and in GitHub Actions CI.
# Checks diffs for secrets, credentials, and private IP addresses.
set -euo pipefail

FAILED=0

PATTERNS=(
  # Private IPv4 ranges (RFC 1918)
  '192\.168\.[0-9]{1,3}\.[0-9]{1,3}'
  '10\.[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}'
  '172\.(1[6-9]|2[0-9]|3[01])\.[0-9]{1,3}\.[0-9]{1,3}'
  # AWS credentials
  'AKIA[0-9A-Z]{16}'
  'aws_secret_access_key\s*=\s*[A-Za-z0-9/+=]{40}'
  # GitHub tokens
  'ghp_[A-Za-z0-9]{36}'
  'ghs_[A-Za-z0-9]{36}'
  'github_pat_[A-Za-z0-9_]{82}'
  # Generic API / bearer tokens
  'sk-[A-Za-z0-9]{20,}'
  'Bearer [A-Za-z0-9._~+/=-]{20,}'
  # Inline secrets in assignment style (key = "value")
  '(password|passwd|secret|api_key|api_token|auth_token)\s*[:=]\s*"[^"]{4,}"'
  '(password|passwd|secret|api_key|api_token|auth_token)\s*[:=]\s*'"'"'[^'"'"']{4,}'"'"
  # Redis connection strings with credentials
  'redis://[^@]+@'
  # SSH private key headers
  '-----BEGIN (RSA|EC|OPENSSH|DSA|PGP) PRIVATE KEY'
)

scan_diff() {
  local diff_content="$1"
  local source_label="$2"

  if [[ -z "$diff_content" ]]; then
    return
  fi

  # Only look at added lines (lines starting with +, excluding the +++ file header)
  local added_lines
  added_lines=$(echo "$diff_content" | grep -E '^\+' | grep -v '^\+\+\+' || true)

  if [[ -z "$added_lines" ]]; then
    return
  fi

  for pattern in "${PATTERNS[@]}"; do
    local matches
    matches=$(echo "$added_lines" | grep -P "$pattern" 2>/dev/null || true)
    if [[ -n "$matches" ]]; then
      echo "LEAK DETECTED in $source_label"
      echo "  Pattern: $pattern"
      echo "$matches" | head -3 | sed 's/^/  /'
      echo ""
      FAILED=1
    fi
  done
}

if [[ "${GITHUB_ACTIONS:-}" == "true" ]]; then
  # CI mode: scan the diff of the commits being tested
  BASE="${GITHUB_BASE_REF:-}"
  if [[ -n "$BASE" ]]; then
    # Pull request: scan diff against base branch
    git fetch origin "$BASE" --depth=1 2>/dev/null || true
    CI_DIFF=$(git diff "origin/$BASE"..HEAD 2>/dev/null || true)
    scan_diff "$CI_DIFF" "PR diff vs $BASE"
  else
    # Push: scan last commit
    CI_DIFF=$(git diff HEAD~1..HEAD 2>/dev/null || true)
    scan_diff "$CI_DIFF" "last commit"
  fi
else
  # Local mode: scan staged changes (pre-commit) and unpushed commits (pre-push)
  STAGED=$(git diff --cached 2>/dev/null || true)
  scan_diff "$STAGED" "staged changes"

  UPSTREAM=$(git rev-parse --abbrev-ref --symbolic-full-name '@{upstream}' 2>/dev/null || true)
  if [[ -n "$UPSTREAM" ]]; then
    UNPUSHED=$(git diff "$UPSTREAM"..HEAD 2>/dev/null || true)
    scan_diff "$UNPUSHED" "unpushed commits vs $UPSTREAM"
  fi
fi

if [[ "$FAILED" -eq 1 ]]; then
  echo "---"
  echo "Commit/push BLOCKED: sensitive data detected in diff."
  echo "Remove the flagged content before proceeding."
  echo "To update scan rules: .githooks/scan.sh"
  exit 1
fi

echo "Leak scan: OK"
