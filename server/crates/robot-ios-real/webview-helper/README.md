# Optional iOS WebView inspection

Native accessibility works without this helper. DOM inspection requires a trusted,
unlocked device, Web Inspector, and an app that enables `WKWebView.isInspectable`.

The native release archive includes this directory beside `device-bot`. Install
Node `^22.22.2`, `^24.15.0` or `>=26`, then run in this directory:

```sh
npm ci --omit=dev
```

Keep `package-lock.json` to install the verified dependency graph. The helper
reports a missing runtime/dependency explicitly; it never substitutes native AX
results for an explicit DOM query. `DEVICE_BOT_IOS_WEBVIEW_HELPER` can override
the script location. Simulator discovery uses the exact selected UDID through
`NativeSimctl`; there is no global socket fallback.

DOM element activation uses a document-bound reference and JavaScript focus/click,
not a trusted native pointer event. It rejects detached/replaced nodes and covered
elements in the selected document. Apps that require trusted pointer events need
native accessibility or a separately verified WebView screen-coordinate mapping.
DOM rectangles are viewport CSS pixels; they must not be used as iOS screen points.
Native screen overlay for these DOM rows is intentionally unavailable until that
mapping is verified. Cross-origin iOS frames return an explicit unavailable error.

Chrome iframe coordinate inspection supports untranslated/unscaled content and
translation. Scale, skew, rotation and ancestor CSS zoom are rejected explicitly.
YAML stores stable app/page/frame selectors, never runtime element references.

Run `npm test` for the helper's context/action regression tests in a source checkout.
