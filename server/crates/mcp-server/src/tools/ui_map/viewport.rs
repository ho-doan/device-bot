//! Identify a macOS scroll container and fingerprint its visible slice.
use std::hash::{DefaultHasher, Hash, Hasher};

use robot_core::{Bounds, Point, UiElement};

pub(super) const MAX_STEPS_PER_DIRECTION: u8 = 4;
pub(super) const SCROLL_SETTLE_MS: u64 = 180;

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
    anchors: Vec<(String, [i32; 4])>,
}

#[derive(Clone)]
pub(super) struct Reveal {
    pub anchor: Viewport,
    pub delta: i32,
    pub slices: Vec<(u64, Viewport)>,
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
        ((i64::from(self.bounds.bottom) - i64::from(self.bounds.top)) * 7 / 10).clamp(80, 900)
            as i32
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
        self.same_container(other)
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
}

pub(super) fn inspect(elements: &[UiElement], flutter: bool) -> Result<Option<Viewport>, ()> {
    let kind = if flutter {
        "scrollView "
    } else {
        "AXScrollArea"
    };
    let matches_kind = |e: &UiElement| {
        if flutter {
            e.class_name.starts_with(kind)
        } else {
            e.class_name == kind
        }
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
    let id = first
        .resource_id
        .as_ref()
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
    let mut anchors = elements
        .iter()
        .filter(intersects)
        .filter_map(|e| {
            let id = e
                .resource_id
                .as_ref()
                .filter(|id| e.clickable && !id.is_empty())?;
            let r = &e.bounds;
            Some((id.clone(), [r.left, r.top, r.right, r.bottom]))
        })
        .collect::<Vec<_>>();
    anchors.sort_unstable();
    let mut visible = elements
        .iter()
        .filter(intersects)
        .map(|e| {
            let r = &e.bounds;
            (
                &e.class_name,
                &e.resource_id,
                e.text
                    .as_deref()
                    .filter(|_| e.element_type == Some(robot_core::ElementType::StaticText)),
                (r.left, r.top, r.right, r.bottom),
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
        bounds: b.clone(),
        slice: hash.finish(),
        anchors,
    }))
}
