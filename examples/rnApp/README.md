# Orbit — React Native login example

A clone of [`examples/flutter_app`](../flutter_app) with the same demo
authentication, routing, checklist, and responsive workspace.

## Run

```sh
cd examples/rnApp
npm install
npm start
```

Demo credentials: **alex@orbit.demo** / **Orbit123!**.
Use **Try demo workspace** to fill the credentials and sign in immediately.

Behavior source of truth: [../flutter_app/BEHAVIOR_FLOWS.md](../flutter_app/BEHAVIOR_FLOWS.md).

For Device Bot tap/input automation on Android, use the dedicated
[React Native profile and flow](../automation_test/README.md#react-native-android-fields-and-keyboard).
It handles the email suggestion popup and keeps controls above the keyboard.

## Check

```sh
npm test
```
