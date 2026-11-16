# Plan: 200 Test Cases (Comprehensive Coverage)

**Current status (2026-11-16):** 38 cases (22 existing + 16 new edge cases)
**Target:** 200 total cases
**Gap:** 162 remaining cases

## Distribution Strategy

| Category | Cases | Examples | Effort per case | Est. Days |
|----------|-------|----------|-----------------|-----------|
| **QC Live (24)** | 6→24 | Unicode variants, long recording edge cases, element selection boundary, concurrent recording | 0.5-1 day | 12-24 |
| **Import/Export (12)** | 6→12 | YAML variant combinations, circular imports, very large files | 0.5 day | 6 |
| **Report/Persistence (12)** | 4→12 | Project/case combinations, orphaned evidence, history stress | 0.5-1 day | 6-12 |
| **Web/Auth/Client (15)** | 8→15 | Auth variants, endpoint types, connection recovery patterns | 0.5 day | 7-10 |
| **Platform Support (30)** | 0→30 | Android real + simulator, iOS Wi-Fi + USB, Windows driver, Chrome variants (headless, incognito, profiles) | 2-3 days | 60-90 |
| **Performance/Load (20)** | 0→20 | Soak tests (100+ cases/project), concurrent users, large flows (1000+ steps), memory/CPU monitoring | 1-2 days | 20-40 |
| **Accessibility (15)** | 0→15 | Screen reader, keyboard-only workflows, large text (200%), high contrast, focus management | 1 day | 15 |
| **Localization (5)** | 0→5 | Vietnamese UI, RTL layout (Arabic), locale date/time, IME handling | 0.5 day | 2-3 |
| **Integration/CI (21)** | 0→21 | CI pipeline, artifact storage, version compatibility, LAN backend setup | 0.5-1 day | 10-20 |
| **Regression/Bug Database (8)** | 0→8 | Auto-generated from EB-* bugs, once EB-01 fixed | 0.2 day | 1-2 |
| **TOTAL** | **200** | | | **139-207 days** |

## Phase 1: Write 162 Remaining Cases (Parallel tracks)

### Track A: QC Live Extensions (6 → 24 = +18 cases) — ~12 days
1. Unicode variants: CJK, RTL, combining chars, emoji skin tones
2. Text input: empty, whitespace-only, very long (10KB+), special quotes/escapes
3. Element selection: disabled, hidden, overlapped, off-screen, dynamic IDs
4. Scroll: to bottom, to top, out of viewport, rapid back-and-forth
5. Long recording stability: 50 → 100 → 200 actions, verify performance
6. Evidence dialog: rapid open/close, rapid transitions
7. Recording autosave: verify every step saved, verify recoverable after crash

### Track B: Import/Export Extensions (6 → 12 = +6 cases) — ~3 days
1. Nested YAML: recipes with sub-recipes, circular references
2. YAML size: 1KB, 10KB, 100KB, 1MB files
3. File path: long paths (>255 chars), special chars in names
4. Export variants: minified, with-comments, sorted vs unsorted
5. Round-trip: import → export → import → compare

### Track C: Report/Persistence (4 → 12 = +8 cases) — ~8 days
1. Project size: 1, 10, 100, 1000 cases per project
2. History queries: filter by date, status, app_id, concurrent access
3. Orphaned evidence: missing PNG, missing trace file, partial history
4. Concurrent publishes: 2+ runners → same project → atomic writes
5. Report backup/recovery: corrupt database, missing files, permission denied
6. Evidence storage: cloud storage (S3), local filesystem, network paths

### Track D: Web/Auth (8 → 15 = +7 cases) — ~5 days
1. Auth: expired token, revoked token, token refresh, 2FA, SAML
2. Endpoints: localhost:3337, 192.168.1.x:3337, domain.com, public DNS
3. Connection: HTTP (insecure), HTTPS, WebSocket upgrade, Proxy
4. Multi-tab scenarios: same token, different tokens, session hijacking
5. Recovery: graceful disconnect, network interrupt, reconnect same session
6. Device list updates: add device mid-session, remove device, device rename
7. Roles/permissions: read-only, admin, cross-project access, scope isolation

### Track E: Platform Support (0 → 30 = +30 cases) — ~60-90 days [DEFER to Phase 2]
1. **Android** (10 cases):
   - Real device (USB) + Simulator (AVD) — ADB connection stability
   - App foreground/background, screen on/off, device rotate
   - Gesture: swipe, pinch, long-press, multi-touch
   - Input: IME variants (Gboard, SwiftKey), clipboard, voice input
   - Capture: fullscreen, crop region, permission prompts
   - Performance: 4K recording, low-end device, battery drain measurement

2. **iOS** (8 cases):
   - Simulator + real iPhone (Wi-Fi + USB)
   - App lifecycle: foreground/background, terminate, relaunch
   - Safari WebDriver capability coverage
   - XCUITest bridge reliability (macOS host only)
   - Gesture: swipe, long-press, 3D-touch (if supported)
   - Input: custom keyboards, voice dictation, clipboard

3. **Windows** (7 cases):
   - Desktop app control via Win32 UI automation
   - Chrome Remote Desktop connection setup
   - Long-lived session stability (8+ hours)
   - Window management: minimize, snap, fullscreen, multi-monitor
   - Input: keyboard layout variants (dvorak, colemak), IME (Chinese/Japanese)
   - Performance: CPU/memory baseline, thermal throttling impact

4. **macOS** (5 cases):
   - Native app (desktop app) + Web (Chrome)
   - Universal Binary (Apple Silicon vs Intel) compatibility
   - Screen capture: GraphicsBuffer API, permission prompts
   - Gesture: trackpad multi-touch (pinch, rotation)
   - Accessibility: voice control, switch control, keyboard navigation

### Track F: Performance/Load (0 → 20 = +20 cases) — ~40 days [DEFER]
1. **Soak tests** (5):
   - 24-hour continuous recording/replay cycle
   - 1000+ runs per project, concurrent queries
   - Memory leak detection, connection pool stability
   - Network flapping (disconnect/reconnect every 5s for 1h)

2. **Concurrency** (5):
   - 10+ browser tabs on same backend
   - 5 concurrent mobile devices (Android, iOS, Chrome)
   - 2 report queries + 1 runner + 1 import all simultaneously
   - Lock contention, race conditions in history writes

3. **Large data** (10):
   - Flow with 1000 steps, 100KB screenshots per step
   - 10,000-element DOM (selector query performance)
   - Project with 10K+ cases, report pagination
   - Video recording 4K@60fps for 10+ minutes, file > 1GB
   - Text input: 10KB+ continuous, multi-line with 1000+ lines

### Track G: Accessibility (0 → 15 = +15 cases) — ~15 days [DEFER]
1. **Screen reader** (6):
   - NVDA (Windows), JAWS (Windows), VoiceOver (macOS/iOS)
   - QC web: button labels, form fields, semantic tree, status announcements
   - Report: project list, case list, evidence modal navigation
   - Verify text-only mode works (no images required)

2. **Keyboard navigation** (4):
   - Tab order, focus indicators, arrow keys for list navigation
   - Esc key to close modal, Enter to confirm
   - QC Live: keyboard-only recording (no mouse)
   - Report: keyboard-only filtering, image gallery nav via arrow keys

3. **Visual** (5):
   - Large text (200%, 300%), high contrast mode
   - Color blindness (protanopia, deuteranopia, tritanopia)
   - Zoom level extremes (50%, 400%)
   - Dynamic text sizing, responsive layout at narrow widths

### Track H: Localization (0 → 5 = +5 cases) — ~3 days [DEFER]
1. Vietnamese UI (current focus)
2. Arabic (RTL layout test)
3. Simplified Chinese (IME, long strings)
4. Date/time formatting per locale
5. Number formatting (decimal separator, thousand separator)

### Track I: Integration/CI (0 → 21 = +21 cases) — ~15 days [DEFER]
1. **CI pipeline** (7):
   - GitHub Actions matrix (macOS, Windows, Ubuntu)
   - Artifact upload/retention
   - Flaky test detection and retry
   - Performance regression tracking
   - Coverage report aggregation

2. **Version compatibility** (7):
   - Previous device-bot version + current web
   - Current device-bot version + previous web (backward compat)
   - Rust CLI vs Flutter CLI vs web
   - MCP protocol version negotiation

3. **Environment setup** (7):
   - Docker container (Linux), package manager (Homebrew/Chocolatey)
   - Offline installation, air-gapped network
   - Custom backend port, custom report store path
   - Proxy configuration (HTTP, HTTPS, SOCKS)

### Track J: Regression/Bug Database (0 → 100 = +100 cases) [PHASE 2+]
Once EB-* bugs are fixed, auto-generate test cases to prevent regression:
- EB-01 (summary render in dialog): Test each error type (syntax, version, unknown action)
- EB-02 (Live sequential): Template for future "unstable when chained" bugs
- Each fixed bug → 1-5 regression test case variants

---

## Phase 2: Multi-Platform & Performance (96 cases, ~120 days) [DEFERRED]

**Prerequisites:**
- Phase 1 complete (38 + 66 = 104 cases)
- EB-01 bug fixed (summary render)
- EB-02 diagnosed (Live stability)
- CI infrastructure ready (separate Windows/Android/iOS runners)

**Timeline:** Week 3-6 (after Phase 1)

---

## Immediate Action (Next 3 days)

**Phase 1, Tracks A-D:** Write 66 additional cases to reach **104 total (38 + 66)**

### Day 1-2: QC Live Extensions + Import/Export (18 + 6 = 24 cases)
- Create YAML templates for text input variants, element selection boundary cases
- Generate fixture files for various file sizes and formats
- Update `run.py` CASES dict with new entries

### Day 3: Report & Auth Extensions (8 + 7 = 15 cases)
- Create report fixtures (large project, empty project, orphaned evidence scenarios)
- Create auth test endpoints (expired token, multi-tab session, permission variants)
- Update harness to support new fixture types

### Post-Phase-1 Review:
- Run full 104-case suite
- Fix failures, document EB-* bugs
- Measure CI runtime (estimate days for Phases 2-3)
- Update coverage metrics

---

## Long-term (8+ weeks)

**Phase 2:** Multi-platform + Performance (96 cases, ~8 weeks)
- Android real device + simulator (10 cases)
- iOS simulator + real device (8 cases)
- Windows desktop + remote (7 cases)
- Performance/soak tests (20 cases)
- Accessibility (15 cases)
- Regression suite (36 cases from fixed bugs)

**Phase 3+:** Integration, localization, automation (100+ cases, ongoing)
