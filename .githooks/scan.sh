#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
/opt/codex/wg-manager-docker/dev/tmp/pre-push-secret-scan.sh "$repo_root"
