#!/usr/bin/env bash
# IT-03 | password visibility preserves text; IME Next focuses password and Done submits.
set -euo pipefail
readonly CASE_ID=IT-03
source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/support/mcp.sh"
assert_text 'Show password'
fill_field 'Email address' 'alex@orbit.demo'
press_ime_action next
assert_focused_field 'Password'
type_text 'Orbit123!'
password_value='Orbit123!'
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
find_element 'Password' true true field
press_ime_action done
assert_text 'A fresh start, Alex.'
finish_flow
