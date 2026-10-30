#!/usr/bin/env bash
# IT-06 | reset validates email and leaves demo credentials unchanged.
set -euo pipefail
readonly CASE_ID=IT-06
source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/support/mcp.sh"
tap_control 'Forgot password?'
assert_text 'Forgot your password?'
assert_absent 'Show password'
# Type and text are applied before maxResults, so one extra slot exposes a second field.
assert_text_field_count 1
tap_control 'Try password reset'
assert_text 'Enter a valid email address.'
fill_field 'Email address' 'someone@example.org'
press_ime_action done
note_limitation 'The 700 ms reset busy state and disabled controls cannot be reliably observed through sequential native MCP queries.'
assert_text 'You’re all set.'
assert_text 'Reset request simulated. This demo does not send email or change your password.'
screenshot reset-complete
tap_control 'Back to sign in'
email_value=''; password_value=''
enter_credentials 'alex@orbit.demo' 'Orbit123!'
tap_control 'Sign in'
assert_text 'A fresh start, Alex.'
finish_flow
