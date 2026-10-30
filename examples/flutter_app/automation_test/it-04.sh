#!/usr/bin/env bash
# IT-04 | failed login preserves credentials and can be retried.
set -euo pipefail
readonly CASE_ID=IT-04
source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/support/mcp.sh"
enter_credentials 'alex@orbit.demo' 'incorrect'
note_limitation 'The 700 ms busy state, disabled controls and duplicate-submit request count cannot be reliably observed through sequential native MCP queries.'
tap_control 'Sign in'
assert_text 'Email or password is incorrect. Try the demo account below.'
assert_field_value 'Email address' 'alex@orbit.demo'
tap_control 'Show password'
assert_field_value 'Password' 'incorrect'
screenshot failed-login
tap_control 'Hide password'
fill_field 'Password' 'Orbit123!'
tap_control 'Sign in'
assert_text 'A fresh start, Alex.'
finish_flow
