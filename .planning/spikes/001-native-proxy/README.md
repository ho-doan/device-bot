---
spike: 001
name: native-proxy
type: comparison
validates: "A test client retains its API URL while an isolated relay injects and clears observable HTTP and WebSocket faults."
verdict: PARTIAL
related: []
tags: [proxy, native, loopback]
---

# Native proxy routes

This spike uses only synthetic fixtures. It does not integrate a native driver,
change the existing Chrome implementation, or authorize per-device network
settings. The relay logs whether the traffic was origin-form, absolute-form, or
inside a CONNECT negotiation.

## Intended topology — devices call the host's IPv4 address

```text
Device client → HOST_LAN_IPV4:8080 [reverse relay] → 127.0.0.1:8080 [API on host]
Host control → 127.0.0.1:8080 [same API, direct]
```

The device does not call its own loopback. If the API listens only on host
loopback, the relay can listen on the host's LAN IPv4 address on the **same
port**. No API port change and no client URL change are needed when clients
already use that LAN endpoint. In the split-bind fixture on macOS, both
listeners coexisted. The LAN path saw the 503, delayed response, transport
drop, WebSocket rejection/cut, and recovery, while the direct host API
returned 200 with the 503 rule active. Physical iPhone Safari independently
completed the same-port LAN suite with HTTP 200 → 503 → 200, an abort at
658 ms in the delay case, transport drop, and WebSocket rejection, cut and
recovery.

If the existing API owns the LAN address and port, or uses a wildcard bind
that covers it, treat that as a collision. On this macOS host a normal bind
failed with `EADDRINUSE` against an active wildcard listener. However, an
address-specific listener using `SO_REUSEADDR` could bind and then receive
traffic addressed to that LAN IP instead of the wildcard listener. Do not
rely on successful bind as proof that the port was free. Split mode disables
address reuse. Check the actual listener before launch and avoid changing
the upstream automatically.
A direct host control is not evidence of device-to-device isolation. Any
device using the same LAN listener can see a listener-wide rule unless the
deployment explicitly scopes the policy to the test client or reserves a
separate listener or test network.

## Results

Except for the physical iPhone LAN tests, the first-pass results below
exercised literal-loopback **client** URLs as comparison cases. Their
bypass behavior and ADB workarounds are not prerequisites for LAN clients.

| Target | Observed result | Limit |
|---|---|---|
| Android 36 emulator | `adb reverse` delivered the unchanged `127.0.0.1:8080` URL, with HTTP 200 → 503 → 200, timeout, transport drop, WebSocket handshake refusal, live socket cut and recovery. A separate direct route stayed 200. | The mapping is device-and-port scoped, not app scoped. No physical Android was attached. Require an unused port and preserve existing ADB mappings. |
| iOS 27 simulator | Native URLSession HTTP CONNECT configuration bypassed literal loopback. A reverse relay bound at `127.0.0.1:8080` with the fixture upstream on `:18080` passed all five faults and recovery. | This simulator shared the Mac loopback in this test. Reserving the port does not isolate two apps or simulators on the same host. |
| Physical iPhone | Safari on an iPhone 15 Pro Max used the same LAN endpoint throughout the run. It observed 200 → 503 → 200, a 659 ms client timeout, drop, handshake refusal, a cut after a valid WebSocket frame and successful reconnection. | A LAN reverse relay is not a transparent proxy for a native app bound to the phone's `127.0.0.1`. No app-specific device route or independent on-phone control was exercised. |
| macOS | Native URLSession bypassed literal loopback in both HTTP CONNECT and legacy proxy modes. The same native client passed all five faults and recovery when the relay bound to `:8080` and upstream ran at `:18080`. A direct upstream control stayed 200. | On a shared host any client using the same listener can be affected. If the real API already binds that address and port, the relay cannot bind there too. |
| Windows | Both .NET Framework HttpWebRequest and WinHTTP bypassed their configured forward proxy for literal loopback (all 200 and zero hits). Reverse relay held the URL fixed, returned 503 and cut HTTP connections in both stacks, induced a .NET timeout, and passed the .NET ClientWebSocket reject/cut/recovery sequence. | WinHTTP still returned 200 on the delayed request despite a confirmed hit. A listen-port relay does not isolate other processes on the same Windows host. |

## Investigation trail

1. The Chrome listener currently requires a CONNECT or an absolute HTTP URI.
   ADB forwards ordinary origin-form HTTP and WebSocket Upgrade. A production
   port relay needs its own explicitly bound listener policy and must not
   relax target validation on the existing Chrome forward listener.
2. Seeing a healthy response while a fault is armed is ambiguous. For the
   Apple forward tests, the direct fixture recorded the requests and the proxy
   recorded no hits. Moving fixture upstream to a distinct socket instead made
   the same logical target exercise the proxy. On iOS Simulator the probe was
   run as a target-compiled command with `simctl spawn` after a scratch GUI
   wrapper exited during launch.
3. The first physical Safari run reached the relay and recovered WebSocket,
   but final HTTP exceeded the initial 650 ms browser deadline. Keeping 650 ms
   only for the timeout case and allowing 2.2 s for healthy requests produced
   a complete rerun. A client retry may make a single fault step hit more than
   once. Verify at least one hit rather than one exact hit.
4. The Windows forward fixture exercised HTTP status, delay and drop rules
   with zero hits in each stack. With the reverse relay, all five HTTP rows
   hit in both stacks. .NET retried the drop (two hits), and the WinHTTP
   delay case still returned 200. The direct control stayed 200 in every row.
   All four .NET WebSocket rows recorded one hit each.
5. None of these runs exercise TLS termination, whole-device offline,
   production backends, or teardown by a Device Bot Project. The isolated spike
   process was driven by a separate fixture control, not by YAML.

## Reproduce

Run commands from this spike directory. Do not use a real active API as the
fixture upstream. Run one suite at a time against a given relay.

### Android Emulator

Start `python3 -u fixture.py` on the Mac. Build with Android SDK platform 35,
Build Tools 36, and JDK 17. Set `ANDROID_SDK_ROOT` and `JAVA_HOME` and run
`sh build-android.sh` to obtain the APK path. Use a disposable emulator, replace
`SERIAL` with its identifier, and check for existing mappings first.

```sh
adb -s SERIAL reverse --list
adb -s SERIAL reverse --no-rebind tcp:8080 tcp:18765
adb -s SERIAL reverse --no-rebind tcp:18081 tcp:18766
adb -s SERIAL reverse --no-rebind tcp:18082 tcp:8080
adb -s SERIAL install /path/to/Probe.apk
adb -s SERIAL shell am start -W -n dev.devicebot.proxyspike/.ProbeActivity
adb -s SERIAL logcat -d -s NativeProxySpike:I '*:S'
```

Remove only the three mappings created by the spike or shut down the
disposable emulator. The UI and log both print the result.

### macOS and iOS Simulator

```sh
work=$(mktemp -d)
swiftc -module-cache-path "$work/swift-cache" -parse-as-library Probe.swift -o "$work/mac"
xcrun --sdk iphonesimulator swiftc -parse-as-library -swift-version 5 \
  -module-cache-path "$work/swift-cache" -target arm64-apple-ios17.0-simulator \
  -sdk "$(xcrun --sdk iphonesimulator --show-sdk-path)" Probe.swift -o "$work/sim"
codesign --force --sign - "$work/sim"
```

With `python3 -u fixture.py` running, run `"$work/mac"` and
`xcrun simctl spawn SIMULATOR_UDID "$work/sim"` separately. The default uses
HTTP CONNECT. Set `SPIKE_STYLE=legacy` on macOS to compare the legacy proxy
dictionary. Expect the fault cases to look healthy and to have no proxy hits.

Stop that fixture, then run `python3 -u fixture.py --api 18080 --proxy 8080` and
try the origin-form relay. The original test URL remains `127.0.0.1:8080` and
the direct control bypasses the relay at `:18080`.

```sh
SPIKE_STYLE=direct SPIKE_HEALTHY=127.0.0.1:18080 "$work/mac"
SIMCTL_CHILD_SPIKE_STYLE=direct SIMCTL_CHILD_SPIKE_HEALTHY=127.0.0.1:18080 \
  xcrun simctl spawn SIMULATOR_UDID "$work/sim"
```

### iPhone on the test LAN

```sh
python3 -u fixture.py --lan-front MAC_LAN_IP --api 8080 --proxy 8080 \
  --token RANDOM_TEST_TOKEN --front-page phone.html
```

Open `http://MAC_LAN_IP:8080/RANDOM_TEST_TOKEN/` in Safari on the unlocked
iPhone. This uses the split-bind topology without requiring an iOS install.
An independent host request to `http://127.0.0.1:8080/items` uses the API
directly, while requests to `http://MAC_LAN_IP:8080/items` go through the relay.

For the original separate-port comparison:

```sh
python3 -u fixture.py --lan MAC_LAN_IP --api 18980 --proxy 18965 \
  --control 18966 --token RANDOM_TEST_TOKEN --front-page phone.html
```

On the unlocked iPhone, open
`http://MAC_LAN_IP:18965/RANDOM_TEST_TOKEN/` in Safari. The page shows its
result and the relay logs a `browser-result` event. Shut down the fixture
afterward. This experiment does not require installing a native iPhone app.

### Windows

On the Windows host, from a session that can run local scripts, run
`powershell -NoProfile -File windows.ps1` and then
`powershell -NoProfile -File windows.ps1 -Relay`. Use a free port 8080.
Both listeners are loopback-only and stop after each suite. The script reports
the per-stack proxy hit deltas and writes JSON reports in the Windows
temporary directory. `-Relay` additionally runs the four .NET ClientWebSocket
states.

## Signal for implementation

For devices already calling a host LAN IPv4 endpoint, start with an opt-in
host LAN reverse relay and an explicit loopback upstream. Test bind ownership,
access from each device, policy isolation, confirmed hits, and teardown under
an unattended run. ADB reverse is a separate fallback only for Android apps
that really call on-device loopback. The literal-loopback Apple and Windows
relay variants still need a dedicated runtime with a reservable listener.
Do not change system proxy, VPN or certificate settings as a fallback.

## Research

- [Android ADB command reference](https://android.googlesource.com/platform/packages/modules/adb/+/refs/heads/main/docs/user/adb.1.md)
- [Apple URLSessionConfiguration](https://developer.apple.com/documentation/Foundation/URLSessionConfiguration)
- [Apple per-app VPN routing](https://developer.apple.com/documentation/networkextension/routing-your-vpn-network-traffic)
- [Windows per-client HTTP proxy](https://learn.microsoft.com/en-us/dotnet/framework/network-programming/requesting-data)
