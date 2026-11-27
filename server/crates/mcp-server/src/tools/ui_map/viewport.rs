//! Identify a native scroll container and fingerprint its visible slice.
use std::hash::{DefaultHasher, Hash, Hasher};

use robot_core::{Bounds, Point, UiElement};

pub(super) const MAX_STEPS_PER_DIRECTION: u8 = 4;
pub(super) const SCROLL_SETTLE_MS: u64 = 180;

#[derive(Clone, Copy)]
enum ScrollKind {
    Ax,
    Flutter,
    Android,
    Ios,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Locator {
    pub class_name: String,
    pub resource_id: Option<String>,
}

#[derive(Clone)]
pub(super) struct Viewport {
    pub locator: Locator,
    pub bounds: Bounds,
    slice: u64,
    anchors: Vec<(String, [i64; 4])>,
    kind: ScrollKind,
}

#[derive(Clone)]
pub(super) struct Reveal {
    pub anchor: Viewport,
    pub delta: i32,
    pub slices: Vec<(u64, Viewport)>,
}

pub(super) struct ViewportSwipe {
    pub from: Point,
    pub to: Point,
    pub duration_ms: u64,
}

impl Viewport {
    pub fn center(&self) -> Point {
        Point {
            x: (f64::from(self.bounds.left) + f64::from(self.bounds.right)) / 2.,
            y: (f64::from(self.bounds.top) + f64::from(self.bounds.bottom)) / 2.,
        }
    }

    pub fn contains(&self, point: Point) -> bool {
        let b = &self.bounds;
        point.x >= f64::from(b.left)
            && point.x < f64::from(b.right)
            && point.y >= f64::from(b.top)
            && point.y < f64::from(b.bottom)
    }

    pub fn step(&self) -> i32 {
        let height = i64::from(self.bounds.bottom) - i64::from(self.bounds.top);
        match self.kind {
            // Android uses the direction of this delta to choose the
            // viewport-relative gesture. iOS starts at the viewport center.
            ScrollKind::Android => (height * 40 / 100) as i32,
            ScrollKind::Ios => (height * 35 / 100).clamp(1, 900) as i32,
            _ => (height * 7 / 10).clamp(80, 900) as i32,
        }
    }

    pub fn android_swipe(&self, dy: i32) -> Option<ViewportSwipe> {
        if !matches!(self.kind, ScrollKind::Android) {
            return None;
        }
        // A swipe starting at the center can land on a focused editor
        // instead of scrolling. Use the measured viewport's lower-to-upper
        // span (and its reverse), with both ends inside the container.
        let (from, to) = if dy > 0 { (0.8, 0.4) } else { (0.4, 0.8) };
        Some(ViewportSwipe {
            from: Point { x: 0.5, y: from },
            to: Point { x: 0.5, y: to },
            duration_ms: 420,
        })
    }

    pub fn screen_point(&self, relative: Point) -> Point {
        let b = &self.bounds;
        let (left, top) = (f64::from(b.left), f64::from(b.top));
        Point {
            x: left + relative.x * (f64::from(b.right) - left),
            y: top + relative.y * (f64::from(b.bottom) - top),
        }
    }

    pub fn same_container(&self, other: &Self) -> bool {
        let (a, b) = (&self.bounds, &other.bounds);
        self.locator == other.locator
            && a.left.abs_diff(b.left) <= 1
            && a.top.abs_diff(b.top) <= 1
            && a.right.abs_diff(b.right) <= 1
            && a.bottom.abs_diff(b.bottom) <= 1
    }

    pub fn same_position(&self, other: &Self) -> bool {
        self.same_container(other) && self.same_replay_anchor(other)
    }

    /// A relaunched window can be on another display. Verify the same slice
    /// by its content and viewport-local positions, not global screen pixels.
    pub fn same_replay_anchor(&self, other: &Self) -> bool {
        let (a, b) = (&self.bounds, &other.bounds);
        self.locator == other.locator
            && a.right.abs_diff(a.left).abs_diff(b.right.abs_diff(b.left)) <= 1
            && a.bottom.abs_diff(a.top).abs_diff(b.bottom.abs_diff(b.top)) <= 1
            && (self.slice == other.slice
                || !self.anchors.is_empty()
                    && self.anchors.len() == other.anchors.len()
                    && self.anchors.iter().zip(&other.anchors).all(|(a, b)| {
                        a.0 == b.0 && a.1.iter().zip(b.1).all(|(a, b)| a.abs_diff(b) <= 1)
                    }))
    }

    pub fn same_layout(&self, other: &Self) -> bool {
        self.same_container(other) && self.slice == other.slice
    }

    pub fn layout_signature(&self) -> u64 {
        self.slice
    }
}

pub(super) fn inspect(elements: &[UiElement], flutter: bool) -> Result<Option<Viewport>, ()> {
    let kind = if flutter {
        ScrollKind::Flutter
    } else {
        ScrollKind::Ax
    };
    inspect_kind(elements, kind)
}

pub(super) fn inspect_android(elements: &[UiElement]) -> Result<Option<Viewport>, ()> {
    inspect_kind(elements, ScrollKind::Android)
}

pub(super) fn inspect_ios(elements: &[UiElement]) -> Result<Option<Viewport>, ()> {
    inspect_kind(elements, ScrollKind::Ios)
}

fn stable_id(e: &UiElement, kind: ScrollKind) -> Option<&String> {
    e.resource_id
        .as_ref()
        .filter(|id| !matches!(kind, ScrollKind::Ios) || !super::crawl::is_xctest_node_id(id))
}

fn inspect_kind(elements: &[UiElement], kind: ScrollKind) -> Result<Option<Viewport>, ()> {
    let matches_kind = |e: &UiElement| match kind {
        ScrollKind::Flutter => e.class_name.starts_with("scrollView "),
        ScrollKind::Ax => e.class_name == "AXScrollArea",
        ScrollKind::Ios => e.class_name == "scrollView",
        ScrollKind::Android => matches!(
            e.class_name.as_str(),
            "android.widget.ScrollView"
                | "androidx.core.widget.NestedScrollView"
                | "android.widget.ListView"
        ),
    };
    let area = |b: &Bounds| {
        let width = i64::from(b.right) - i64::from(b.left);
        let height = i64::from(b.bottom) - i64::from(b.top);
        (width >= 120 && height >= 100).then(|| i128::from(width) * i128::from(height))
    };
    let mut candidates = elements
        .iter()
        .filter(|e| matches_kind(e))
        .filter_map(|e| Some((area(&e.bounds)?, e)))
        .collect::<Vec<_>>();
    candidates.sort_unstable_by_key(|row| std::cmp::Reverse(row.0));
    let Some(&(_, first)) = candidates.first() else {
        return Ok(None);
    };
    // Do not guess between independent panes or scroll a nested text editor.
    let b = &first.bounds;
    if candidates.iter().skip(1).any(|(_, e)| {
        let c = &e.bounds;
        c == b || c.left < b.left || c.top < b.top || c.right > b.right || c.bottom > b.bottom
    }) {
        return Err(());
    }
    let id = stable_id(first, kind)
        .filter(|id| !id.is_empty() && id.len() <= 160 && !id.chars().any(char::is_control));
    let unique = elements
        .iter()
        .filter(|e| {
            e.class_name.contains(&first.class_name)
                && id.is_none_or(|id| e.resource_id.as_ref() == Some(id))
        })
        .count()
        == 1;
    if !unique {
        return Err(());
    }
    let intersects = |e: &&UiElement| {
        let r = &e.bounds;
        r.right > b.left && r.left < b.right && r.bottom > b.top && r.top < b.bottom
    };
    let relative = |r: &Bounds| {
        [
            i64::from(r.left) - i64::from(b.left),
            i64::from(r.top) - i64::from(b.top),
            i64::from(r.right) - i64::from(b.left),
            i64::from(r.bottom) - i64::from(b.top),
        ]
    };
    let mut anchors = elements
        .iter()
        .filter(intersects)
        .filter_map(|e| {
            let id = stable_id(e, kind).filter(|id| e.clickable && !id.is_empty())?;
            Some((id.clone(), relative(&e.bounds)))
        })
        .collect::<Vec<_>>();
    anchors.sort_unstable();
    let mut visible = elements
        .iter()
        .filter(intersects)
        .map(|e| {
            (
                &e.class_name,
                stable_id(e, kind),
                e.text
                    .as_deref()
                    .filter(|_| e.element_type == Some(robot_core::ElementType::StaticText)),
                relative(&e.bounds),
            )
        })
        .collect::<Vec<_>>();
    visible.sort_unstable();
    let mut hash = DefaultHasher::new();
    visible.hash(&mut hash);
    Ok(Some(Viewport {
        locator: Locator {
            class_name: first.class_name.clone(),
            resource_id: id.cloned(),
        },
        bounds: *b,
        slice: hash.finish(),
        anchors,
        kind,
    }))
}
