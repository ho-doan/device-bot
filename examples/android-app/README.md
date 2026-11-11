# Orbit — Android Compose login example

A clone of [`examples/flutter_app`](../flutter_app) with the same demo
authentication, routing, checklist, and responsive workspace.

## Run

Open `examples/android-app` in Android Studio, or:

```sh
cd examples/android-app
./gradlew :app:testDebugUnitTest
./gradlew :app:assembleDebug
```

Demo credentials: **alex@orbit.demo** / **Orbit123!**.
Use **Try demo workspace** to fill the credentials and sign in immediately.

Behavior source of truth: [../flutter_app/BEHAVIOR_FLOWS.md](../flutter_app/BEHAVIOR_FLOWS.md).

Device Bot tap/input smoke uses
[`../automation_test/targets-compose.yaml`](../automation_test/targets-compose.yaml)
with the shared `control-smoke.yaml`. Build/install this fixture before running:
the root exports Compose test tags as native resource IDs. See the
[automation guide](../automation_test/README.md#android-compose-fields).
