# Spike Manifest

## Idea

Test how Android, iOS Simulator, physical iOS, Windows and macOS can route the
same application endpoint through a test fault relay before implementing
native support in Device Bot.

## Requirements

- Do not change the target application or production API to enable a mode
  advertised as transparent.
- Preserve the application endpoint within each run and prove faults hit the
  intended route. A response from the direct control is not a proxy hit.
- Do not change system-wide network settings or trust stores.
- Distinguish platform or stack feasibility from implemented Project support.

## Spikes

| # | Name | Type | Validates | Verdict | Tags |
|---|---|---|---|---|---|
| 001 | [Native proxy routes](001-native-proxy/README.md) | comparison | Fixed endpoint, observed faults, recovery and isolation boundaries | PARTIAL | proxy, native, loopback |
