#!/usr/bin/env python3
"""Add remaining 123 cases to run.py CASES dict."""

from pathlib import Path
import re

ROOT = Path(__file__).parent
RUN_PY = ROOT / "run.py"

# All remaining case names
REMAINING_CASES = [
    # Track E: Multi-platform (30)
    ("android-device-real-boot", {}),
    ("android-device-simulator-boot", {}),
    ("android-app-foreground", {}),
    ("android-app-background", {}),
    ("android-screen-rotate", {}),
    ("android-gesture-swipe", {}),
    ("android-gesture-pinch", {}),
    ("android-gesture-long-press", {}),
    ("android-input-ime-gboard", {}),
    ("android-input-ime-swiftkey", {}),
    ("ios-simulator-boot", {}),
    ("ios-device-wifi", {}),
    ("ios-device-usb", {}),
    ("ios-app-foreground", {}),
    ("ios-app-background", {}),
    ("ios-gesture-swipe", {}),
    ("ios-gesture-long-press", {}),
    ("ios-input-keyboard", {}),
    ("ios-web-safari", {}),
    ("windows-desktop-app", {}),
    ("windows-chrome-remote", {}),
    ("windows-window-minimize", {}),
    ("windows-window-snap", {}),
    ("windows-multimonitor", {}),
    ("windows-input-keyboard-dvorak", {}),
    ("windows-input-ime-chinese", {}),
    ("windows-performance-baseline", {}),
    ("macos-native-app", {}),
    ("macos-universal-binary", {}),
    ("macos-screenshot-api", {}),
    ("macos-gesture-trackpad", {}),
    ("macos-accessibility-voice-control", {}),
    ("macos-keyboard-navigation", {}),
    
    # Track F: Performance (20, shortened to 15)
    ("perf-soak-24h-recording", {}),
    ("perf-soak-24h-replay", {}),
    ("perf-large-project-1000-cases", {}),
    ("perf-large-flow-1000-steps", {}),
    ("perf-concurrent-10-browsers", {}),
    ("perf-concurrent-5-devices", {}),
    ("perf-concurrent-report-query", {}),
    ("perf-dom-10000-elements", {}),
    ("perf-text-input-10kb", {}),
    ("perf-video-recording-1gb", {}),
    ("perf-network-flapping-1h", {}),
    ("perf-memory-leak-detection", {}),
    ("perf-connection-pool-stability", {}),
    ("perf-history-writes-atomic", {}),
    ("perf-cache-eviction", {}),
    
    # Track G: Accessibility (15)
    ("a11y-screen-reader-nvda", {}),
    ("a11y-screen-reader-jaws", {}),
    ("a11y-screen-reader-voiceover", {}),
    ("a11y-keyboard-tab-order", {}),
    ("a11y-keyboard-navigation", {}),
    ("a11y-keyboard-esc-close", {}),
    ("a11y-keyboard-enter-confirm", {}),
    ("a11y-large-text-200p", {}),
    ("a11y-large-text-300p", {}),
    ("a11y-high-contrast-mode", {}),
    ("a11y-color-blind-protanopia", {}),
    ("a11y-color-blind-deuteranopia", {}),
    ("a11y-color-blind-tritanopia", {}),
    ("a11y-zoom-50p", {}),
    ("a11y-zoom-400p", {}),
    
    # Track H: Localization (5)
    ("l10n-vietnamese-ui", {}),
    ("l10n-arabic-rtl", {}),
    ("l10n-chinese-ime", {}),
    ("l10n-date-time-formatting", {}),
    ("l10n-number-formatting", {}),
    
    # Track I: Integration (21)
    ("ci-github-actions-matrix", {}),
    ("ci-artifact-upload", {}),
    ("ci-artifact-retention", {}),
    ("ci-flaky-test-detection", {}),
    ("ci-performance-regression", {}),
    ("ci-coverage-report", {}),
    ("ci-docker-container", {}),
    ("ci-homebrew-install", {}),
    ("ci-chocolatey-install", {}),
    ("ci-offline-installation", {}),
    ("ci-custom-backend-port", {}),
    ("ci-custom-report-store", {}),
    ("ci-proxy-http", {}),
    ("ci-proxy-https", {}),
    ("ci-proxy-socks", {}),
    ("ci-version-compat-old-server", {}),
    ("ci-version-compat-old-web", {}),
    ("ci-version-compat-cli", {}),
    ("ci-mcp-protocol-version", {}),
    ("ci-backend-recovery", {}),
    ("ci-report-store-recovery", {}),
    
    # Track J: Regression (34)
    ("regression-eb01-summary-syntax-error", {}),
    ("regression-eb01-summary-bad-version", {}),
    ("regression-eb01-summary-unknown-action", {}),
    ("regression-eb02-live-suite-v1", {}),
    ("regression-eb02-live-suite-v2", {}),
    ("regression-eb02-live-suite-v3", {}),
    ("regression-unicode-export-literal", {}),
    ("regression-unicode-export-escape", {}),
    ("regression-webrtc-cleanup-race", {}),
    ("regression-webrtc-renderer-dispose", {}),
    ("regression-webrtc-ice-timeout", {}),
    ("regression-webrtc-ice-completion", {}),
    ("regression-input-after-close", {}),
    ("regression-input-kill-app", {}),
    ("regression-evidence-url-encoding", {}),
    ("regression-evidence-case-routing", {}),
    ("regression-fixture-orbit-material", {}),
    ("regression-fixture-orbit-password", {}),
    ("regression-fixture-orbit-avatar", {}),
    ("regression-fixture-orbit-sign-out", {}),
    ("regression-report-download-race", {}),
    ("regression-report-download-retry", {}),
    ("regression-chrome-frame-replace", {}),
    ("regression-selector-duplicates", {}),
    ("regression-selector-disabled", {}),
    ("regression-selector-geometry", {}),
    ("regression-selector-truncation", {}),
    ("regression-selector-fallback-position", {}),
    ("regression-step-timeout-deadline", {}),
    ("regression-step-reconnect-limit", {}),
    ("regression-auth-missing", {}),
    ("regression-auth-wrong", {}),
    ("regression-auth-endpoint-scoping", {}),
    ("regression-auth-bearer-delete", {}),
]

# Read run.py
content = RUN_PY.read_text()

# Find the closing brace of CASES dict
match = re.search(r'(\s*)\}(\s+def select_cases)', content, re.DOTALL)
if not match:
    raise RuntimeError("Could not find CASES dict closing brace")

indent = match.group(1)
before_closing = content[:match.start()]

# Generate new entries
new_entries = []
new_entries.append(f'\n{indent}# Phase 2-3 Extensions (125 new cases) — Tracks E-J')

for case_name, config in REMAINING_CASES:
    config_str = ", ".join(f'"{k}": {repr(v)}' if isinstance(v, (int, str, bool, type(None))) else f'"{k}": {v}' for k, v in config.items())
    if config_str:
        new_entries.append(f'{indent}"{case_name}": {{{config_str}}},')
    else:
        new_entries.append(f'{indent}"{case_name}": {{}},')

# Reconstruct run.py
new_content = before_closing + '\n'.join(new_entries) + f'\n{indent}}}\n\n' + content[match.start() + len(match.group(0)) - len(match.group(1)) - 1:]

RUN_PY.write_text(new_content)
print(f"✅ Added {len(REMAINING_CASES)} cases to run.py")
print(f"   Total should be: 75 (Phase 1) + {len(REMAINING_CASES)} (Phase 2-3) = {75 + len(REMAINING_CASES)}")
