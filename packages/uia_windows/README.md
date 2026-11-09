# uia_windows (compatibility)

Use [`device_bot_flutter`](../device_bot_flutter/README.md) for new integrations.
This package retains the old `UiaWindows.start()` / `stop()` API as a deprecated
alias. Native discovery now runs first; widget-tree discovery is a fallback.
