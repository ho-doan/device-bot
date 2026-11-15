# Device Bot icons

![Device Bot](device-bot.svg)

`device-bot.svg` is the editable source: a robot face on navy, with teal
controls. `orbit-demo.svg` is the generated amber variant for example apps.
The original brand assets use the repository's [Apache-2.0 license](../../LICENSE).

Run from the repository root:

```sh
uv run scripts/generate_icons.py
```

The script declares its Python dependencies inline. It renders PNG, ICO and
ICNS files without requiring Flutter, Xcode, a system Cairo library, or an
image-generation service. Generated assets are committed so app builds do not
need this tool. Change the SVG, rerun, and review the resulting assets together.

| Destination | Variant / format |
| --- | --- |
| Repository README + reusable artwork | Teal SVG and 1024px PNG |
| QC Companion | Teal macOS/iOS catalogs, Windows ICO, Android PNGs, web/PWA icons, header PNG |
| QA Reports | Teal embedded SVG, favicon and header mark |
| iPhone Broadcast app | Teal iOS asset catalog |
| Flutter, SwiftUI, React Native examples | Amber native asset catalogs and launcher icons |
| Compose example | Amber legacy launcher icons and adaptive foreground with navy background |
| WPF example | Amber application/window ICO |
| React/Vite example | Amber SVG favicon |
| Electron/Vue example | Amber runtime PNG and packaged PNG/ICO/ICNS |

Apple mobile/store icons and maskable web icons are opaque. The symbol stays
inside the central safe area; the Compose adaptive foreground is inset further.
Apple catalog sizes/scales and existing filenames are preserved where present.
Rebuild/reinstall native apps to refresh OS icon caches. GitHub's repository
avatar belongs to the owner account; the README displays the repository icon
without changing that account avatar.
