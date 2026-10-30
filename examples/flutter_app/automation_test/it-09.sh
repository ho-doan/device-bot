#!/usr/bin/env bash
# IT-09 | sign out hides account data and login restores the requested account.
set -euo pipefail
readonly CASE_ID=IT-09
source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/support/mcp.sh"
sign_in_demo
tap_control 'View your profile'
assert_text 'Alex Lane'
tap_control 'Sign out'
assert_text 'Welcome back.'
assert_absent 'Alex Lane'
assert_absent 'A fresh start, Alex.'
screenshot signed-out
note_limitation 'Direct protected /home and /account GoRouter requests and from query assertions are not available through current MCP tools; these security checks remain in Flutter.'
email_value=''; password_value=''
sign_in_demo 'Alex Lane'
assert_text 'alex@orbit.demo'
finish_flow
