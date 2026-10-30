#!/usr/bin/env bash
# IT-08 | checklist progress, navigation tabs and profile.
set -euo pipefail
readonly CASE_ID=IT-08
source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/support/mcp.sh"
sign_in_demo
assert_text '1/3'
assert_checked 'Make space for a fresh start' true
tap_control 'Make space for a fresh start'
assert_text '0/3'
assert_checked 'Make space for a fresh start' false
tap_control 'Choose your most important task'
tap_control 'Take a mindful break'
tap_control 'Make space for a fresh start'
assert_text '3/3'
assert_checked 'Make space for a fresh start' true
assert_checked 'Choose your most important task' true
assert_checked 'Take a mindful break' true
assert_text '20 min'
assert_text 'Today'
screenshot completed-checklist
note_limitation 'LinearProgressIndicator.value and TextDecoration.lineThrough are not exposed by native elements; checkbox checked state is verified.'
tap_control 'Account'
assert_text 'Alex Lane'
assert_text 'alex@orbit.demo'
tap_control 'Overview'
# Re-entering Home creates fresh checklist state, as specified in the Flutter test.
assert_text '1/3'
tap_control 'View your profile'
assert_text 'Alex Lane'
tap_control 'Overview'
# The IconButton tooltip is Your account, but the native tree exposes its
# CircleAvatar child as the enabled AL button on the app bar.
tap_account_avatar
assert_text 'Alex Lane'
finish_flow
