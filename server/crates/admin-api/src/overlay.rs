//! Draw QC-style element labels onto a screenshot PNG.
//!
//! The Flutter inspector paints `#index name` chips over the live image. This
//! module burns the same labels into the PNG so MCP and admin clients receive
//! them from the server.

use std::{io::Cursor, path::PathBuf, sync::OnceLock};

use ab_glyph::{Font, FontVec, GlyphId, PxScale, ScaleFont};
use font8x8::UnicodeFonts;
use image::{ExtendedColorType, ImageEncoder, Rgba, RgbaImage};
use robot_core::{Platform, Robot, RobotError, UiElement};

const LABEL_SIZE: f32 = 16.0;
const LABEL_MAX_WIDTH: i32 = 280;
const BORDER: [u8; 4] = [255, 179, 0, 255];
const LABEL_FILL: [u8; 4] = [17, 17, 17, 204];
const LABEL_TEXT: [u8; 4] = [255, 255, 255, 255];

pub struct AnnotatedScreenshot {
    pub bytes: Vec<u8>,
    pub annotated: bool,
}

pub struct OverlayFrame {
    pub platform: Option<Platform>,
    pub origin_x: i32,
    pub origin_y: i32,
    /// `native` or `flutter` when the tree source is known. Included in the YAML hint.
    pub source: Option<String>,
}

/// Capture-independent compositor. An element-list or decode failure leaves the
/// original screenshot unchanged so inspection still has an image.
pub async fn with_element_labels(
    robot: &dyn Robot,
    device_id: &str,
    png: Vec<u8>,
    display_id: Option<&str>,
) -> Result<AnnotatedScreenshot, RobotError> {
    let FoundElements { elements, source } = match inspection_elements(robot, device_id).await {
        Ok(found) if !found.elements.is_empty() => found,
        _ => {
            return Ok(AnnotatedScreenshot {
                bytes: png,
                annotated: false,
            });
        }
    };
    let mut origin_x = 0;
    let mut origin_y = 0;
    if let Some(display_id) = display_id {
        if let Ok(displays) = robot.list_displays(device_id).await {
            if let Some(display) = displays
                .into_iter()
                .find(|display| display.id == display_id)
            {
                origin_x = display.bounds.left;
                origin_y = display.bounds.top;
            }
        }
    }
    let platform = robot.list_devices().await.ok().and_then(|devices| {
        devices
            .into_iter()
            .find(|device| device.id == device_id)
            .map(|device| device.platform)
    });
    match annotate_png(
        &png,
        &elements,
        &OverlayFrame {
            platform,
            origin_x,
            origin_y,
            source: Some(source),
        },
    ) {
        Ok(bytes) => Ok(AnnotatedScreenshot {
            bytes,
            annotated: true,
        }),
        Err(_) => Ok(AnnotatedScreenshot {
            bytes: png,
            annotated: false,
        }),
    }
}

async fn inspection_elements(
    robot: &dyn Robot,
    device_id: &str,
) -> Result<FoundElements, RobotError> {
    let mut elements = robot.list_elements(device_id).await?;
    let source = if elements.is_empty() {
        elements = robot.list_flutter_elements(device_id).await?;
        "flutter"
    } else {
        "native"
    };
    robot_core::prepare_elements(&mut elements);
    Ok(FoundElements {
        elements,
        source: source.to_owned(),
    })
}

struct FoundElements {
    elements: Vec<UiElement>,
    source: String,
}

pub fn annotate_png(
    png: &[u8],
    elements: &[UiElement],
    frame: &OverlayFrame,
) -> Result<Vec<u8>, String> {
    let mut image = image::load_from_memory(png)
        .map_err(|error| error.to_string())?
        .to_rgba8();
    let scale = pixel_scale(
        image.width(),
        image.height(),
        elements,
        frame.platform.as_ref(),
    );
    for (offset, element) in elements.iter().enumerate() {
        draw_element(&mut image, element, offset + 1, frame, scale);
    }
    let hints = flow_hints(elements, frame.source.as_deref());
    encode_png(&extend_with_hints(image, &hints))
}

/// Match the QC inspector: iOS accessibility bounds are points, screenshots are pixels.
pub fn pixel_scale(
    width: u32,
    height: u32,
    elements: &[UiElement],
    platform: Option<&Platform>,
) -> f64 {
    if !matches!(platform, Some(Platform::Ios)) || elements.is_empty() || width == 0 || height == 0
    {
        return 1.0;
    }
    let max_right = elements
        .iter()
        .map(|element| element.bounds.right)
        .max()
        .unwrap_or(0);
    let max_bottom = elements
        .iter()
        .map(|element| element.bounds.bottom)
        .max()
        .unwrap_or(0);
    if max_right <= 0 || max_bottom <= 0 {
        return 1.0;
    }
    let x_scale = f64::from(width) / f64::from(max_right);
    if !x_scale.is_finite() || x_scale <= 0.0 {
        return 1.0;
    }
    [1.0, 2.0, 3.0]
        .into_iter()
        .min_by(|left, right| {
            (left - x_scale)
                .abs()
                .partial_cmp(&(right - x_scale).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(1.0)
}

fn draw_element(
    image: &mut RgbaImage,
    element: &UiElement,
    index: usize,
    frame: &OverlayFrame,
    scale: f64,
) {
    let left = scale_axis(element.bounds.left - frame.origin_x, scale);
    let top = scale_axis(element.bounds.top - frame.origin_y, scale);
    let right = scale_axis(element.bounds.right - frame.origin_x, scale);
    let bottom = scale_axis(element.bounds.bottom - frame.origin_y, scale);
    if right - left < 2 || bottom - top < 2 {
        return;
    }
    stroke_rect(image, left, top, right, bottom, 2, BORDER);
    let label = element_label(element, index);
    let (text_width, text_height) = measure_text(&label);
    let chip_width = (text_width + 10).clamp(24, LABEL_MAX_WIDTH);
    let chip_height = text_height + 8;
    let width = image.width() as i32;
    let height = image.height() as i32;
    let chip_left = left.clamp(0, (width - chip_width).max(0));
    let preferred_top = top - chip_height - 2;
    let chip_top = (if preferred_top >= 0 {
        preferred_top
    } else {
        top
    })
    .clamp(0, (height - chip_height).max(0));
    fill_rect(
        image,
        chip_left,
        chip_top,
        chip_left + chip_width,
        chip_top + chip_height,
        LABEL_FILL,
    );
    draw_text(image, &label, chip_left + 5, chip_top + 4, chip_width - 10);
}

/// YAML fields robot-flow can match: resource_id, label, position, class_contains,
/// element_type, and source. Prefer a unique id or label. Otherwise add position
/// among the same matches so the hint selects this element.
fn flow_hints(elements: &[UiElement], source: Option<&str>) -> Vec<Vec<String>> {
    elements
        .iter()
        .enumerate()
        .map(|(offset, _element)| {
            let mut lines = vec![format!("#{}", offset + 1)];
            if let Some(source) = source.filter(|value| !value.is_empty()) {
                lines.push(format!("source: {source}"));
            }
            lines.extend(selector_lines(elements, offset));
            lines
        })
        .collect()
}

fn selector_lines(elements: &[UiElement], index: usize) -> Vec<String> {
    let element = &elements[index];
    let resource_id = element
        .resource_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let label = visible_label(element);
    if let Some(resource_id) = resource_id {
        if count_resource_id(elements, resource_id) == 1 {
            return vec![format!("resource_id: {}", yaml_quote(resource_id))];
        }
    }
    if let Some(label) = &label {
        if count_label(elements, label) == 1 {
            return vec![format!("label: {}", yaml_quote(label))];
        }
    }
    if let Some(resource_id) = resource_id {
        let position = count_resource_id(&elements[..=index], resource_id);
        return vec![
            format!("resource_id: {}", yaml_quote(resource_id)),
            format!("position: {position}"),
        ];
    }
    if let Some(label) = label {
        let position = count_label(&elements[..=index], &label);
        return vec![
            format!("label: {}", yaml_quote(&label)),
            format!("position: {position}"),
        ];
    }
    let mut lines = vec![format!(
        "class_contains: {}",
        yaml_quote(&element.class_name)
    )];
    if let Some(kind) = element.element_type {
        lines.push(format!("element_type: {}", kind.as_str()));
    }
    let position = elements[..=index]
        .iter()
        .filter(|candidate| {
            candidate.class_name.contains(&element.class_name)
                && candidate.element_type == element.element_type
        })
        .count();
    lines.push(format!("position: {position}"));
    lines
}

fn visible_label(element: &UiElement) -> Option<String> {
    [
        element.text.as_deref(),
        element.content_description.as_deref(),
    ]
    .into_iter()
    .flatten()
    .flat_map(str::lines)
    .map(str::trim)
    .find(|line| !line.is_empty())
    .map(str::to_owned)
}

fn count_resource_id(elements: &[UiElement], resource_id: &str) -> usize {
    elements
        .iter()
        .filter(|element| element.resource_id.as_deref().map(str::trim) == Some(resource_id))
        .count()
}

fn count_label(elements: &[UiElement], label: &str) -> usize {
    let needle = label.to_lowercase();
    elements
        .iter()
        .filter(|element| {
            [
                element.text.as_deref(),
                element.content_description.as_deref(),
            ]
            .into_iter()
            .flatten()
            .flat_map(str::lines)
            .any(|line| line.trim().to_lowercase() == needle)
        })
        .count()
}

fn yaml_quote(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace(['\n', '\r'], " ")
    )
}

fn extend_with_hints(shot: RgbaImage, hints: &[Vec<String>]) -> RgbaImage {
    if hints.is_empty() {
        return shot;
    }
    let title = "YAML flow";
    let line_height = 22i32;
    let pad = 16;
    let mut widest = measure_text(title).0;
    let mut rows = 1;
    for hint in hints {
        rows += hint.len() + 1;
        for line in hint {
            widest = widest.max(measure_text(line).0);
        }
    }
    let panel_width = (widest + pad * 2).clamp(240, 640);
    let panel_height = pad * 2 + rows as i32 * line_height;
    let mut canvas = RgbaImage::from_pixel(
        shot.width() + panel_width as u32,
        shot.height().max(panel_height as u32),
        Rgba([17, 24, 39, 255]),
    );
    for (x, y, pixel) in shot.enumerate_pixels() {
        canvas.put_pixel(x, y, *pixel);
    }
    let origin_x = shot.width() as i32;
    for y in 0..canvas.height() {
        paint(&mut canvas, origin_x, y as i32, BORDER);
    }
    let mut cursor_y = pad;
    draw_text(
        &mut canvas,
        title,
        origin_x + pad,
        cursor_y,
        panel_width - pad * 2,
    );
    cursor_y += line_height;
    for hint in hints {
        for line in hint {
            draw_text(
                &mut canvas,
                line,
                origin_x + pad,
                cursor_y,
                panel_width - pad * 2,
            );
            cursor_y += line_height;
        }
        cursor_y += line_height;
    }
    canvas
}

fn element_label(element: &UiElement, index: usize) -> String {
    let name = [
        element.text.as_deref(),
        element.content_description.as_deref(),
        element.resource_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .find(|value| !value.is_empty())
    .map(str::to_owned)
    .unwrap_or_else(|| format!("{} #{index}", element.class_name));
    let mut label = format!("#{index} {}", name.replace(['\n', '\r'], " "));
    let max_chars = 48;
    if label.chars().count() > max_chars {
        label = label.chars().take(max_chars.saturating_sub(1)).collect();
        label.push('…');
    }
    label
}

fn scale_axis(value: i32, scale: f64) -> i32 {
    (f64::from(value) * scale).round() as i32
}

fn stroke_rect(
    image: &mut RgbaImage,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    stroke: i32,
    color: [u8; 4],
) {
    let width = image.width() as i32;
    let height = image.height() as i32;
    let x0 = left.clamp(0, width);
    let x1 = right.clamp(0, width);
    let y0 = top.clamp(0, height);
    let y1 = bottom.clamp(0, height);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    for x in x0..x1 {
        for offset in 0..stroke {
            paint(image, x, y0 + offset, color);
            paint(image, x, y1 - 1 - offset, color);
        }
    }
    for y in y0..y1 {
        for offset in 0..stroke {
            paint(image, x0 + offset, y, color);
            paint(image, x1 - 1 - offset, y, color);
        }
    }
}

fn fill_rect(image: &mut RgbaImage, left: i32, top: i32, right: i32, bottom: i32, color: [u8; 4]) {
    let width = image.width() as i32;
    let height = image.height() as i32;
    for y in top.clamp(0, height)..bottom.clamp(0, height) {
        for x in left.clamp(0, width)..right.clamp(0, width) {
            paint(image, x, y, color);
        }
    }
}

fn paint(image: &mut RgbaImage, x: i32, y: i32, color: [u8; 4]) {
    if x < 0 || y < 0 {
        return;
    }
    let (x, y) = (x as u32, y as u32);
    if x >= image.width() || y >= image.height() || color[3] == 0 {
        return;
    }
    let pixel = image.get_pixel_mut(x, y);
    if color[3] == 255 {
        *pixel = Rgba(color);
        return;
    }
    let alpha = u32::from(color[3]);
    let inverse = 255 - alpha;
    for channel in 0..3 {
        pixel.0[channel] = ((u32::from(color[channel]) * alpha
            + u32::from(pixel.0[channel]) * inverse)
            / 255) as u8;
    }
    pixel.0[3] = 255;
}

fn measure_text(text: &str) -> (i32, i32) {
    if let Some(font) = system_font() {
        let scaled = font.as_scaled(PxScale::from(LABEL_SIZE));
        let width = text
            .chars()
            .map(|glyph| advance(&scaled, glyph))
            .sum::<f32>();
        return (width.ceil() as i32, LABEL_SIZE.ceil() as i32);
    }
    let columns = text.chars().count() as i32;
    (columns * 16, 16)
}

fn draw_text(image: &mut RgbaImage, text: &str, x: i32, y: i32, max_width: i32) {
    let mut cursor = x;
    let limit = x + max_width.max(0);
    if let Some(font) = system_font() {
        let scaled = font.as_scaled(PxScale::from(LABEL_SIZE));
        let baseline = y as f32 + scaled.ascent();
        for glyph in text.chars() {
            let step = advance(&scaled, glyph);
            if cursor + step.ceil() as i32 > limit {
                break;
            }
            if scaled.glyph_id(glyph) == GlyphId(0) {
                blit_bitmap(image, glyph, cursor, y);
            } else {
                let positioned = scaled.glyph_id(glyph).with_scale_and_position(
                    PxScale::from(LABEL_SIZE),
                    ab_glyph::point(cursor as f32, baseline),
                );
                if let Some(outlined) = font.outline_glyph(positioned) {
                    let bounds = outlined.px_bounds();
                    outlined.draw(|gx, gy, coverage| {
                        if coverage < 0.2 {
                            return;
                        }
                        let mut color = LABEL_TEXT;
                        color[3] = (coverage * 255.0).round() as u8;
                        paint(
                            image,
                            bounds.min.x.floor() as i32 + gx as i32,
                            bounds.min.y.floor() as i32 + gy as i32,
                            color,
                        );
                    });
                }
            }
            cursor += step.ceil() as i32;
        }
        return;
    }
    for glyph in text.chars() {
        if cursor + 16 > limit {
            break;
        }
        blit_bitmap(image, glyph, cursor, y);
        cursor += 16;
    }
}

fn advance(scaled: &ab_glyph::PxScaleFont<&FontVec>, glyph: char) -> f32 {
    let id = scaled.glyph_id(glyph);
    if id == GlyphId(0) {
        16.0
    } else {
        scaled.h_advance(id).max(1.0)
    }
}

fn blit_bitmap(image: &mut RgbaImage, glyph: char, x: i32, y: i32) {
    let Some(rows) = font8x8::BASIC_FONTS.get(glyph) else {
        return;
    };
    for (row, bits) in rows.iter().enumerate() {
        for column in 0..8 {
            if bits & (1 << column) == 0 {
                continue;
            }
            for dy in 0..2 {
                for dx in 0..2 {
                    paint(
                        image,
                        x + column as i32 * 2 + dx,
                        y + row as i32 * 2 + dy,
                        LABEL_TEXT,
                    );
                }
            }
        }
    }
}

fn system_font() -> Option<&'static FontVec> {
    static FONT: OnceLock<Option<FontVec>> = OnceLock::new();
    FONT.get_or_init(load_system_font).as_ref()
}

fn load_system_font() -> Option<FontVec> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("DEVICE_BOT_OVERLAY_FONT") {
        candidates.push(PathBuf::from(path));
    }
    candidates.extend([
        PathBuf::from("/System/Library/Fonts/Supplemental/Arial Unicode.ttf"),
        PathBuf::from("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"),
        PathBuf::from("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"),
    ]);
    if let Some(windows) = std::env::var_os("WINDIR") {
        candidates.push(PathBuf::from(&windows).join("Fonts/meiryo.ttc"));
        candidates.push(PathBuf::from(windows).join("Fonts/arial.ttf"));
    }
    candidates.iter().find_map(|path| {
        let bytes = std::fs::read(path).ok()?;
        let face = first_face(&bytes)?.to_vec();
        FontVec::try_from_vec(face).ok()
    })
}

fn first_face(bytes: &[u8]) -> Option<&[u8]> {
    if bytes.len() >= 16 && bytes.starts_with(b"ttcf") {
        let count = u32::from_be_bytes(bytes[8..12].try_into().ok()?);
        if count == 0 {
            return None;
        }
        let offset = u32::from_be_bytes(bytes[12..16].try_into().ok()?) as usize;
        return bytes.get(offset..);
    }
    Some(bytes)
}

fn encode_png(image: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut output = Cursor::new(Vec::new());
    image::codecs::png::PngEncoder::new(&mut output)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ExtendedColorType::Rgba8,
        )
        .map_err(|error| error.to_string())?;
    Ok(output.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use robot_core::Bounds;

    fn element(text: &str, left: i32, top: i32, right: i32, bottom: i32) -> UiElement {
        UiElement {
            class_name: "Button".into(),
            resource_id: None,
            text: Some(text.into()),
            content_description: None,
            bounds: Bounds {
                left,
                top,
                right,
                bottom,
            },
            clickable: true,
            enabled: true,
            focused: None,
            checked: None,
            selected: None,
            value: None,
            element_type: None,
        }
    }

    fn white_png(width: u32, height: u32) -> Vec<u8> {
        let image = RgbaImage::from_pixel(width, height, Rgba([255, 255, 255, 255]));
        encode_png(&image).unwrap()
    }

    #[test]
    fn draws_the_element_name_above_its_bounds() {
        let png = white_png(180, 120);
        let annotated = annotate_png(
            &png,
            &[element("Save", 20, 50, 120, 90)],
            &OverlayFrame {
                platform: None,
                origin_x: 0,
                origin_y: 0,
                source: Some("native".into()),
            },
        )
        .unwrap();
        let image = image::load_from_memory(&annotated).unwrap().to_rgba8();
        let border = image.get_pixel(20, 50).0;
        assert!(
            border[0] > 200 && border[1] > 120 && border[2] < 40,
            "{border:?}"
        );
        let chip = image.get_pixel(22, 30).0;
        assert!(chip[0] < 80 && chip[1] < 80 && chip[2] < 80, "{chip:?}");
        let has_text = (24..70)
            .any(|x| (26..46).any(|y| image.get_pixel(x, y).0[0] > chip[0].saturating_add(40)));
        assert!(has_text, "element text was not drawn on the label");
        assert!(
            image.width() > 180,
            "YAML hint panel should widen the image"
        );
        let panel = image.get_pixel(image.width() - 8, 8).0;
        assert!(panel[0] < 40 && panel[2] > panel[0], "{panel:?}");
        assert_ne!(annotated, png);
    }

    #[test]
    fn suggests_the_yaml_fields_the_flow_can_match() {
        let save = element("Save", 0, 0, 10, 10);
        let mut keyed = save.clone();
        keyed.resource_id = Some("save-button".into());
        assert_eq!(
            selector_lines(&[keyed.clone()], 0),
            vec!["resource_id: \"save-button\"".to_owned()]
        );

        let mut duplicate = keyed.clone();
        duplicate.bounds.left = 20;
        let pair = [keyed, duplicate];
        assert_eq!(
            selector_lines(&pair, 1),
            vec![
                "resource_id: \"save-button\"".to_owned(),
                "position: 2".to_owned()
            ]
        );

        let mut second = save.clone();
        second.text = Some("Cancel".into());
        assert_eq!(
            selector_lines(&[save, second], 0),
            vec!["label: \"Save\"".to_owned()]
        );

        let mut plain = element("", 0, 0, 8, 8);
        plain.text = None;
        plain.element_type = Some(robot_core::ElementType::Button);
        let mut other = plain.clone();
        other.bounds.top = 20;
        assert_eq!(
            selector_lines(&[plain, other], 1),
            vec![
                "class_contains: \"Button\"".to_owned(),
                "element_type: button".to_owned(),
                "position: 2".to_owned(),
            ]
        );
    }

    #[test]
    fn scales_ios_point_bounds_onto_the_pixel_screenshot() {
        assert_eq!(
            pixel_scale(
                90,
                90,
                &[element("Save", 0, 0, 20, 20)],
                Some(&Platform::Ios)
            ),
            3.0
        );
        let annotated = annotate_png(
            &white_png(90, 90),
            &[element("Save", 10, 10, 20, 20)],
            &OverlayFrame {
                platform: Some(Platform::Ios),
                origin_x: 0,
                origin_y: 0,
                source: None,
            },
        )
        .unwrap();
        let image = image::load_from_memory(&annotated).unwrap().to_rgba8();
        let scaled_border = image.get_pixel(30, 45).0;
        assert!(
            scaled_border[0] > 200 && scaled_border[2] < 40,
            "{scaled_border:?}"
        );
    }
}
