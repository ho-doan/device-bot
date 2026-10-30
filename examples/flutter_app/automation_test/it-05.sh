#!/usr/bin/env bash
# IT-05 | normalize email while matching password case and whitespace exactly.
set -euo pipefail
readonly CASE_ID=IT-05
source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/support/mcp.sh"
fill_field 'Email address' 'alex@orbit.demo'
for password in 'orbit123!' ' Orbit123! ' 'Orbit123! '; do
  fill_field 'Password' "$password"
  tap_control 'Sign in'
  assert_text 'Email or password is incorrect. Try the demo account below.'
done
screenshot rejected-passwords
enter_credentials '  ALEX@ORBIT.DEMO  ' 'Orbit123!'
tap_control 'Sign in'
assert_text 'A fresh start, Alex.'
finish_flow
