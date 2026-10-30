#!/usr/bin/env bash
# IT-02 | invalid input shows errors and corrected input signs in.
set -euo pipefail
readonly CASE_ID=IT-02
source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/support/mcp.sh"
tap_control 'Sign in'
assert_text 'Enter a valid email address.'
assert_text 'Enter your password.'
assert_text 'Check the highlighted fields to continue.'
screenshot empty-validation
enter_credentials 'not-an-email' 'Orbit123!'
tap_control 'Sign in'
assert_text 'Enter a valid email address.'
assert_absent 'Enter your password.'
screenshot invalid-email
fill_field 'Email address' 'alex@orbit.demo'
tap_control 'Sign in'
assert_text 'A fresh start, Alex.'
finish_flow
