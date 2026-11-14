# Release cache benchmark — 2026-11-14

The Flutter release cache now comes from `master`. SDK entries are separated by
OS, runner architecture, Flutter version and build platform. Pub entries use only
`server/flutter_app/pubspec.lock`, so unrelated example lockfiles do not invalidate
the release dependencies. Release jobs restore the Flutter and Rust caches and
do not save their own tag-scoped copies.

## Measurements

Both runs build and package the same v0.1.18 application code. The benchmark runs
the updated workflow at `48adbeb` using `workflow_dispatch`; it uploads all three
native archives but skips publishing a GitHub release. The Flutter cache warmer
completed on all four runner configurations before the benchmark started.

| Intel macOS step | v0.1.18 release | Cache benchmark |
| --- | ---: | ---: |
| Cargo release build | 4:09 | 3:27 |
| Flutter setup | 1:58 | 1:42 |
| Flutter macOS build | 6:35 | 6:00 |
| Final Flutter + Rust cache post steps | 2:23 | 0:00 |
| Entire Intel job | 18:07 | 14:02 |
| Shared QC web job | 1:50 | 1:08 |
| Entire workflow, including scheduling gaps | 20:23 | 15:20 |

The Intel job shortened by 4:05. Removing final cache writes accounts for 2:23 of
that difference. Flutter setup improved by 16 seconds; its native build still
costs about six minutes. Rust build inputs/cache configuration were unchanged
apart from disabling cache saves, so the shorter Rust compilation is not evidence
of a new compiler-cache optimization.

The workflow duration is 5:03 shorter, but this comparison includes a 14-second
publish job in the original release and no publish job in the benchmark. A
single run on shared hosted runners does not establish a stable expected saving.
The separate one-time Flutter cache warm-up took 4:08 end to end; that cost is
outside the benchmark and recurs when the SDK/platform or dependency keys change.

## Evidence

- [Original v0.1.18 release](https://github.com/ho-doan/device-bot/actions/runs/43604503491)
- [Flutter cache warm-up on master](https://github.com/ho-doan/device-bot/actions/runs/43607304517)
- [Cache benchmark, all builds successful, publish skipped](https://github.com/ho-doan/device-bot/actions/runs/43607988429)
- [Workflow/cache change](https://github.com/ho-doan/device-bot/commit/48adbeb)

## Repeat

1. Complete **Warm release Flutter cache** on `master` after a Flutter version or
   release lockfile change. **Warm release Rust cache** maintains Rust dependencies.
2. Dispatch **Release native tools** on `master`, supplying the tag-form Cargo
   version (for example, `v0.1.18`). Manual dispatch builds and packages without
   publishing; ordinary `v*` tag pushes still publish.
3. Compare the same runner's named build/setup steps, exact SDK/Pub cache hits,
   post-step durations and workflow scheduling separately. Do not count a cold
   warm-up as a warmed release build.
