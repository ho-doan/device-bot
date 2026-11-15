#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["resvg-py>=0.2,<1", "pillow>=11,<13"]
# ///
"""Render the shared SVG into launcher icons. Run: uv run scripts/generate_icons.py."""
import io
import json
from pathlib import Path
import resvg_py
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
BRAND = ROOT / 'assets/brand'
PRODUCT = (BRAND / 'device-bot.svg').read_text()
DEMO = PRODUCT.replace('<title>Device Bot</title>', '<title>Device Bot · Orbit examples</title>').replace('#5EEAD4', '#FDBA74')
BACKGROUND = '#0F172A'
rendered = {}
outputs = set()


def image(svg, size, opaque=False):
    key = (svg, size, opaque)
    if key not in rendered:
        png = resvg_py.svg_to_bytes(svg_string=svg, width=size, height=size)
        result = Image.open(io.BytesIO(png)).convert('RGBA')
        if opaque:
            background = Image.new('RGBA', result.size, BACKGROUND)
            background.alpha_composite(result)
            result = background.convert('RGB')
        rendered[key] = result
    return rendered[key]


def write_icon(path, svg, size, opaque=False):
    path = ROOT / path
    path.parent.mkdir(parents=True, exist_ok=True)
    result = image(svg, size, opaque)
    if path.suffix == '.ico':
        result.save(path, sizes=[(n, n) for n in (16, 24, 32, 48, 64, 128, 256)])
    else:
        result.save(path)
    outputs.add(path)


def apple(catalog, svg):
    path = ROOT / catalog / 'Contents.json'
    data = json.loads(path.read_text())
    for entry in data['images']:
        size = round(float(entry['size'].split('x')[0]) * float(entry.get('scale', '1x')[:-1]))
        if not entry.get('filename'):
            appearance = '-'.join(a['value'] for a in entry.get('appearances', []))
            entry['filename'] = f"icon-{entry['idiom']}-{size}{'-' + appearance if appearance else ''}.png"
        write_icon(Path(catalog) / entry['filename'], svg, size, opaque=entry['idiom'] != 'mac')
    path.write_text(json.dumps(data, indent=2) + '\n')


def android(directory, svg):
    for density, size in [('mdpi', 48), ('hdpi', 72), ('xhdpi', 96), ('xxhdpi', 144), ('xxxhdpi', 192)]:
        for path in (ROOT / directory / f'mipmap-{density}').glob('ic_launcher*'):
            write_icon(path, svg, size, opaque=True)


def flutter(directory, svg):
    app = Path(directory)
    for platform in ('ios', 'macos'):
        apple(app / platform / 'Runner/Assets.xcassets/AppIcon.appiconset', svg)
    android(app / 'android/app/src/main/res', svg)
    write_icon(app / 'windows/runner/resources/app_icon.ico', svg, 256)
    write_icon(app / 'web/favicon.png', svg, 64)
    for size in (192, 512):
        for name in (f'Icon-{size}.png', f'Icon-maskable-{size}.png'):
            write_icon(app / 'web/icons' / name, svg, size, opaque=True)


def icns(path, svg):
    """Build Apple's icon container without requiring macOS iconutil."""
    import struct
    chunks = []
    for code, size in [('icp4', 16), ('icp5', 32), ('icp6', 64), ('ic07', 128), ('ic08', 256), ('ic09', 512), ('ic10', 1024)]:
        png = io.BytesIO()
        image(svg, size).save(png, format='PNG')
        payload = png.getvalue()
        chunks.append(code.encode() + struct.pack('>I', len(payload) + 8) + payload)
    body = b''.join(chunks)
    target = ROOT / path
    target.write_bytes(b'icns' + struct.pack('>I', len(body) + 8) + body)
    outputs.add(target)


def main():
    (BRAND / 'orbit-demo.svg').write_text(DEMO)
    write_icon(BRAND / 'device-bot.png', PRODUCT, 1024)
    flutter('server/flutter_app', PRODUCT)
    write_icon('server/flutter_app/assets/device-bot.png', PRODUCT, 128)
    broadcast = 'server/crates/robot-ios-real/xctest-runner/WifiBroadcast/App/Assets.xcassets'
    catalog = ROOT / broadcast / 'AppIcon.appiconset'
    catalog.mkdir(parents=True, exist_ok=True)
    (ROOT / broadcast / 'Contents.json').write_text(json.dumps({'info': {'author': 'xcode', 'version': 1}}, indent=2) + '\n')
    (catalog / 'Contents.json').write_text(json.dumps({
        'images': [{'idiom': 'universal', 'platform': 'ios', 'size': '1024x1024', 'filename': 'device-bot.png'}],
        'info': {'author': 'xcode', 'version': 1},
    }, indent=2) + '\n')
    apple(catalog, PRODUCT)
    flutter('examples/flutter_app', DEMO)
    apple('examples/swift-app/swift-app/swift-app/Assets.xcassets/AppIcon.appiconset', DEMO)
    apple('examples/rnApp/ios/rnApp/Images.xcassets/AppIcon.appiconset', DEMO)
    android('examples/rnApp/android/app/src/main/res', DEMO)
    android('examples/android-app/app/src/main/res', DEMO)
    # Android adaptive icons need a transparent foreground inside the safe zone.
    foreground = DEMO.replace('  <rect width="1024" height="1024" rx="224" fill="#0F172A"/>', '')
    foreground = foreground.replace('<path ', '<g transform="translate(102.4 102.4) scale(0.8)"><path ', 1).replace('</svg>', '</g></svg>')
    write_icon('examples/android-app/app/src/main/res/drawable-nodpi/device_bot_launcher.png', foreground, 432)
    write_icon('examples/win-app/Resources/app.ico', DEMO, 256)
    write_icon('examples/elec-app/resources/icon.png', DEMO, 512)
    write_icon('examples/elec-app/resources/icon.ico', DEMO, 256)
    icns('examples/elec-app/resources/icon.icns', DEMO)
    (ROOT / 'examples/vite-app/public/favicon.svg').write_text(DEMO)
    (ROOT / 'server/crates/robot-report/src/web/device-bot.svg').write_text(PRODUCT)
    print(f'Rendered {len(outputs)} launcher and web icons from assets/brand/device-bot.svg')


if __name__ == '__main__':
    main()
