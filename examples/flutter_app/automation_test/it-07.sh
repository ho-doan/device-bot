#!/usr/bin/env bash
# IT-07 | account return destination survives reset cancellation and completion.
set -euo pipefail
readonly CASE_ID=IT-07
source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/support/mcp.sh"
note_limitation 'MCP cannot request GoRouter /account directly. Establish the protected account return destination by signing out from Account; cold-start route/query checks remain in Flutter.'
sign_in_demo
tap_control 'Account'
assert_text 'Alex Lane'
tap_control 'Sign out'
assert_text 'Welcome back.'
assert_absent 'Alex Lane'
tap_control 'Forgot password?'
assert_text 'Forgot your password?'
tap_control 'Back to sign in'
assert_text 'Welcome back.'
tap_control 'Forgot password?'
fill_field 'Email address' 'alex@orbit.demo'
tap_control 'Try password reset'
assert_text 'You’re all set.'
tap_control 'Back to sign in'
email_value=''; password_value=''
sign_in_demo 'Alex Lane'
assert_text 'alex@orbit.demo'
finish_flow
