#!/usr/bin/env bash
# IT-01 | cold launch and demo login open the workspace.
set -euo pipefail
readonly CASE_ID=IT-01
source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/support/mcp.sh"
screenshot login
sign_in_demo
assert_text '1/3'
finish_flow
# GoRouter URI and Flutter exceptions remain in the Flutter integration test.
