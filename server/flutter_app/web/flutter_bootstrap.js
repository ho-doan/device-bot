{{flutter_js}}
{{flutter_build_config}}

// QC is served by the native backend on LAN. Keep renderer assets on that
// origin and load the current release without an offline service-worker cache.
_flutter.loader.load({
  config: { canvasKitBaseUrl: 'canvaskit/' },
});
