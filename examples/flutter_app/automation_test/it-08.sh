#!/usr/bin/env bash
# IT-08 | checklist progress, navigation tabs and profile.
set -euo pipefail
readonly CASE_ID=IT-08
source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/support/mcp.sh"
sign_in_demo
assert_text '1/3'
tap_control 'Make space for a fresh start'
assert_text '0/3'
tap_control 'Choose your most important task'
tap_control 'Take a mindful break'
tap_control 'Make space for a fresh start'
assert_text '3/3'
assert_text '20 min'
assert_text 'Today'
screenshot completed-checklist
note_limitation 'Native MCP elements do not expose CheckboxListTile.value, progress-indicator numeric value or text line-through decoration; progress text is checked here.'
tap_control 'Account'
assert_text 'Alex Lane'
assert_text 'alex@orbit.demo'
tap_control 'Overview'
# Re-entering Home creates fresh checklist state, as specified in the Flutter test.
assert_text '1/3'
tap_control 'View your profile'
assert_text 'Alex Lane'
tap_control 'Overview'
tap_control 'Your account'
assert_text 'Alex Lane'
finish_flow
