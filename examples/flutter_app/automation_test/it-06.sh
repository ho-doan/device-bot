#!/usr/bin/env bash
# IT-06 | reset validates email and leaves demo credentials unchanged.
set -euo pipefail
readonly CASE_ID=IT-06
source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/support/mcp.sh"
tap_control 'Forgot password?'
assert_text 'Forgot your password?'
assert_absent 'Show password'
# Only native text fields are checked; do not confuse explanatory text with a field.
query_elements 'Password'
jq -e '[.[] | select(.className|test("EditText|textField";"i"))] | length==0' \
  "$ARTIFACT_DIR/last-query.json" >/dev/null
tap_control 'Try password reset'
assert_text 'Enter a valid email address.'
fill_field 'Email address' 'someone@example.org'
note_limitation 'Reset busy/disabled controls and keyboard Done submission require additional MCP capabilities; this flow uses the reset button.'
tap_control 'Try password reset'
assert_text 'You’re all set.'
assert_text 'Reset request simulated. This demo does not send email or change your password.'
screenshot reset-complete
tap_control 'Back to sign in'
email_value=''; password_value=''
enter_credentials 'alex@orbit.demo' 'Orbit123!'
tap_control 'Sign in'
assert_text 'A fresh start, Alex.'
finish_flow
