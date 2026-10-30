#!/usr/bin/env bash
# IT-03 | password visibility preserves text; keyboard action coverage is limited.
set -euo pipefail
readonly CASE_ID=IT-03
source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/support/mcp.sh"
assert_text 'Show password'
enter_credentials 'alex@orbit.demo' 'Orbit123!'
tap_control 'Show password'
assert_text 'Hide password'
assert_field_value 'Password' 'Orbit123!'
screenshot password-visible
tap_control 'Hide password'
assert_text 'Show password'
# Reveal once more to verify that hiding did not discard or change the value.
tap_control 'Show password'
assert_field_value 'Password' 'Orbit123!'
tap_control 'Hide password'
note_limitation 'IME Next/Done and focus assertions require keyboard-action support; this flow submits with the Sign in button.'
tap_control 'Sign in'
assert_text 'A fresh start, Alex.'
finish_flow
