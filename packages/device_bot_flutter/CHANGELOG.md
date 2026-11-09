## Unreleased

* Export custom suffix/prefix actions in decorated text fields, including
  GestureDetector, InkWell, and semantic buttons, without duplicating nested
  gesture nodes or changing editor hit bounds.

## 1.1.0

* Rename to device_bot_flutter and retain the uia_windows compatibility export.
* Add native IO registration for Android/iOS/macOS and page-local web snapshots.
* Normalize coordinates for each platform; expose Flutter widget types.
* Native discovery remains first; Flutter is used only for missing selectors.

## 1.0.0

- Add a self-contained Flutter Windows widget-tree bridge with PID discovery,
  live control state, viewport filtering and desktop coordinates.
