//! Frame identity, ownership, and viewport mapping for Chrome iframes.
//!
//! Runtime CDP ids stay in this module. YAML only receives [`robot_core::WebContext`].

use std::collections::HashSet;

use robot_core::{Bounds, RobotError, WebContext, WebFrameSelector};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FrameNode {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub url: String,
    pub children: Vec<FrameNode>,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct AttachedTarget {
    pub session_id: String,
    pub parent_session_id: Option<String>,
    pub target_type: String,
    pub root_frame_id: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AuthorizedFrame {
    pub session_id: String,
    pub frame_id: String,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct FrameOwnerBox {
    pub frame_id: String,
    pub css: Option<String>,
    pub name: String,
    pub url: String,
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub unsupported_transform: bool,
}

pub(crate) fn merge_frame_tree(root: &mut FrameNode, replacement: &FrameNode) {
    if root.id == replacement.id {
        root.children = replacement.children.clone();
    } else if replacement.parent_id.as_deref() == Some(root.id.as_str()) {
        if let Some(existing) = root
            .children
            .iter_mut()
            .find(|child| child.id == replacement.id)
        {
            *existing = replacement.clone();
        } else {
            root.children.push(replacement.clone());
        }
    } else {
        for child in &mut root.children {
            merge_frame_tree(child, replacement);
        }
    }
}

pub(crate) fn parse_frame_tree(tree: &Value) -> Result<FrameNode, RobotError> {
    let root = tree.get("frameTree").unwrap_or(tree);
    parse_node(root)
}

pub(crate) fn session_for_frame<'a>(
    root: &FrameNode,
    id: &str,
    authorized: &'a [AuthorizedFrame],
) -> Option<&'a str> {
    frame_chain(root, id).iter().rev().find_map(|frame| {
        authorized
            .iter()
            .find(|candidate| candidate.frame_id == frame.id)
            .map(|candidate| candidate.session_id.as_str())
    })
}

fn parse_node(node: &Value) -> Result<FrameNode, RobotError> {
    let frame = node.get("frame").unwrap_or(node);
    let id = frame["id"].as_str().ok_or_else(|| {
        RobotError::CommandFailed("Chrome frame tree entry has no frame id".into())
    })?;
    let children = node["childFrames"]
        .as_array()
        .map(|children| {
            children
                .iter()
                .map(parse_node)
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();
    Ok(FrameNode {
        id: id.to_owned(),
        parent_id: frame["parentId"].as_str().map(str::to_owned),
        name: frame["name"].as_str().unwrap_or("").to_owned(),
        url: frame["url"].as_str().unwrap_or("").to_owned(),
        children,
    })
}

pub(crate) fn find_frame<'a>(root: &'a FrameNode, frame_id: &str) -> Option<&'a FrameNode> {
    if root.id == frame_id {
        return Some(root);
    }
    for child in &root.children {
        if let Some(found) = find_frame(child, frame_id) {
            return Some(found);
        }
    }
    None
}

pub(crate) fn descendant_frames(root: &FrameNode) -> Vec<&FrameNode> {
    let mut frames = Vec::new();
    fn walk<'a>(node: &'a FrameNode, frames: &mut Vec<&'a FrameNode>) {
        for child in &node.children {
            frames.push(child);
            walk(child, frames);
        }
    }
    walk(root, &mut frames);
    frames
}

pub(crate) fn frame_chain<'a>(root: &'a FrameNode, frame_id: &str) -> Vec<&'a FrameNode> {
    let mut chain = Vec::new();
    fn walk<'a>(node: &'a FrameNode, frame_id: &str, chain: &mut Vec<&'a FrameNode>) -> bool {
        if node.id == frame_id {
            return true;
        }
        for child in &node.children {
            chain.push(child);
            if walk(child, frame_id, chain) {
                return true;
            }
            chain.pop();
        }
        false
    }
    if walk(root, frame_id, &mut chain) {
        chain
    } else {
        Vec::new()
    }
}

pub(crate) fn frame_ids(root: &FrameNode) -> HashSet<String> {
    let mut ids = HashSet::new();
    fn walk(node: &FrameNode, ids: &mut HashSet<String>) {
        ids.insert(node.id.clone());
        for child in &node.children {
            walk(child, ids);
        }
    }
    walk(root, &mut ids);
    ids
}

/// An iframe target is usable only when its root frame belongs to the owned page tree
/// and its parent session is the page or an already authorized descendant.
#[allow(dead_code)]
pub(crate) fn authorize_frames(
    page_session: &str,
    owned_frames: &HashSet<String>,
    attached: &[AttachedTarget],
) -> Vec<AuthorizedFrame> {
    let mut authorized_sessions = HashSet::from([page_session.to_owned()]);
    let mut accepted = Vec::new();
    let mut remaining: Vec<&AttachedTarget> = attached.iter().collect();
    loop {
        let before = remaining.len();
        remaining.retain(|target| {
            let parent_ok = target
                .parent_session_id
                .as_deref()
                .is_some_and(|parent| authorized_sessions.contains(parent));
            let frame_ok = target.target_type == "iframe"
                && owned_frames.contains(&target.root_frame_id)
                && !target.session_id.is_empty();
            if parent_ok && frame_ok {
                authorized_sessions.insert(target.session_id.clone());
                accepted.push(AuthorizedFrame {
                    session_id: target.session_id.clone(),
                    frame_id: target.root_frame_id.clone(),
                });
                false
            } else {
                true
            }
        });
        if remaining.len() == before {
            break;
        }
    }
    accepted
}

pub(crate) fn context_for_frame(
    root: &FrameNode,
    frame_id: &str,
    owners: &[FrameOwnerBox],
) -> Result<WebContext, RobotError> {
    let mut chain = Vec::new();
    fn find<'a>(node: &'a FrameNode, frame_id: &str, chain: &mut Vec<&'a FrameNode>) -> bool {
        if node.id == frame_id {
            return true;
        }
        for child in &node.children {
            chain.push(child);
            if find(child, frame_id, chain) {
                return true;
            }
            chain.pop();
        }
        false
    }
    if root.id == frame_id {
        return Ok(WebContext::default());
    }
    if !find(root, frame_id, &mut chain) {
        return Err(RobotError::InvalidInput(format!(
            "web frame {frame_id} is no longer in this page"
        )));
    }
    let mut path = Vec::new();
    let mut parent_id = root.id.as_str();
    for node in &chain {
        let owner = owners
            .iter()
            .find(|owner| owner.frame_id == node.id)
            .or_else(|| owners.iter().find(|owner| owner_matches_node(owner, node)));
        let selector = if let Some(owner) = owner {
            if owner.unsupported_transform {
                return Err(RobotError::InvalidInput(format!(
                    "frame {} has a rotated or skewed CSS transform; its coordinates are not mapped",
                    node.id
                )));
            }
            selector_from_owner(owner, owners, parent_id)
        } else if !node.name.is_empty() {
            WebFrameSelector {
                name: Some(node.name.clone()),
                ..WebFrameSelector::default()
            }
        } else {
            return Err(RobotError::InvalidInput(format!(
                "frame {} has no stable name or owner selector",
                node.id
            )));
        };
        path.push(selector);
        parent_id = node.id.as_str();
    }
    Ok(WebContext {
        frame_path: path,
        ..WebContext::default()
    })
}

fn owner_matches_node(owner: &FrameOwnerBox, node: &FrameNode) -> bool {
    (!node.name.is_empty() && owner.name == node.name)
        || (!node.url.is_empty() && owner.url == node.url)
}

fn selector_from_owner(
    owner: &FrameOwnerBox,
    owners: &[FrameOwnerBox],
    parent_id: &str,
) -> WebFrameSelector {
    if let Some(css) = owner.css.clone().filter(|css| !css.is_empty()) {
        return WebFrameSelector {
            css: Some(css),
            ..WebFrameSelector::default()
        };
    }
    if !owner.name.is_empty() {
        let count = owners
            .iter()
            .filter(|candidate| candidate.name == owner.name)
            .count();
        return WebFrameSelector {
            name: Some(owner.name.clone()),
            position: (count > 1).then_some(position_among(
                owners,
                |candidate| candidate.name == owner.name,
                owner,
            )),
            ..WebFrameSelector::default()
        };
    }
    let _ = parent_id;
    WebFrameSelector {
        url: Some(owner.url.clone()),
        position: Some(position_among(
            owners,
            |candidate| candidate.url == owner.url,
            owner,
        )),
        ..WebFrameSelector::default()
    }
}

fn position_among(
    owners: &[FrameOwnerBox],
    predicate: impl Fn(&FrameOwnerBox) -> bool,
    owner: &FrameOwnerBox,
) -> u32 {
    owners
        .iter()
        .filter(|candidate| predicate(candidate))
        .position(|candidate| std::ptr::eq(candidate, owner))
        .map(|index| index as u32 + 1)
        .unwrap_or(1)
}

pub(crate) fn bind_owners(children: &[FrameNode], raw: &[Value]) -> Vec<FrameOwnerBox> {
    let mut used = HashSet::new();
    let mut boxes = Vec::new();
    for (index, child) in children.iter().enumerate() {
        let owner = raw.iter().enumerate().find(|(raw_index, owner)| {
            !used.contains(raw_index)
                && ((!child.name.is_empty() && owner["name"].as_str() == Some(child.name.as_str()))
                    || (!child.url.is_empty() && owner["url"].as_str() == Some(child.url.as_str()))
                    || (child.name.is_empty() && child.url.is_empty() && *raw_index == index))
        });
        let Some((raw_index, owner)) = owner else {
            continue;
        };
        used.insert(raw_index);
        boxes.push(FrameOwnerBox {
            frame_id: child.id.clone(),
            css: owner["css"]
                .as_str()
                .filter(|css| !css.is_empty())
                .map(str::to_owned),
            name: owner["name"].as_str().unwrap_or("").to_owned(),
            url: owner["url"].as_str().unwrap_or("").to_owned(),
            left: owner["left"].as_f64().unwrap_or(0.0),
            top: owner["top"].as_f64().unwrap_or(0.0),
            right: owner["right"].as_f64().unwrap_or(0.0),
            bottom: owner["bottom"].as_f64().unwrap_or(0.0),
            unsupported_transform: owner["unsupported"].as_bool().unwrap_or(false),
        });
    }
    boxes
}

pub(crate) fn resolve_frame<'a>(
    root: &'a FrameNode,
    owners: &'a [FrameOwnerBox],
    context: &WebContext,
) -> Result<&'a FrameNode, RobotError> {
    context.validate()?;
    if context.app_id.is_some() {
        return Err(RobotError::InvalidInput(
            "Chrome frame context does not select a native app".into(),
        ));
    }
    let mut node = root;
    for (hop, selector) in context.frame_path.iter().enumerate() {
        let matches: Vec<&FrameNode> = node
            .children
            .iter()
            .filter(|child| frame_matches(child, owners, selector))
            .collect();
        let chosen = choose_match(&matches, selector)
            .map_err(|error| RobotError::InvalidInput(format!("web frame {} {error}", hop + 1)))?;
        node = chosen;
    }
    Ok(node)
}

fn frame_matches(node: &FrameNode, owners: &[FrameOwnerBox], selector: &WebFrameSelector) -> bool {
    let owner = owners.iter().find(|owner| owner.frame_id == node.id);
    if let Some(css) = selector.css.as_deref().filter(|css| !css.is_empty()) {
        return owner.is_some_and(|owner| owner.css.as_deref() == Some(css));
    }
    if let Some(name) = selector.name.as_deref().filter(|name| !name.is_empty()) {
        if node.name != name && owner.is_none_or(|owner| owner.name != name) {
            return false;
        }
    }
    if let Some(url) = selector.url.as_deref().filter(|url| !url.is_empty()) {
        if node.url != url && owner.is_none_or(|owner| owner.url != url) {
            return false;
        }
    }
    selector.css.as_deref().unwrap_or("").is_empty()
        || owner.is_some_and(|owner| owner.css.as_deref() == selector.css.as_deref())
}

fn choose_match<'a>(
    matches: &[&'a FrameNode],
    selector: &WebFrameSelector,
) -> Result<&'a FrameNode, String> {
    match matches {
        [] => Err("was not found".into()),
        [only] if selector.position.is_none() || selector.position == Some(1) => Ok(*only),
        many => {
            let Some(position) = selector.position else {
                return Err(format!(
                    "matches {} owners; set an explicit one-based position",
                    many.len()
                ));
            };
            many.get(position as usize - 1)
                .copied()
                .ok_or_else(|| format!("position {position} is outside the {} matches", many.len()))
        }
    }
}

pub(crate) fn map_bounds(
    local: Bounds,
    owners: &[FrameOwnerBox],
    chain: &[&FrameNode],
) -> Result<Bounds, RobotError> {
    let mut origin_x = 0.0;
    let mut origin_y = 0.0;
    for node in chain {
        let owner = owners
            .iter()
            .find(|owner| owner.frame_id == node.id)
            .ok_or_else(|| {
                RobotError::InvalidInput(format!(
                    "frame {} has no owner box, so its coordinates cannot be mapped",
                    node.id
                ))
            })?;
        if owner.unsupported_transform {
            return Err(RobotError::InvalidInput(format!(
                "frame {} has an unsupported CSS transform or zoom; its coordinates are not mapped",
                node.id
            )));
        }
        origin_x += owner.left;
        origin_y += owner.top;
    }
    Ok(Bounds {
        left: local.left + origin_x.round() as i32,
        top: local.top + origin_y.round() as i32,
        right: local.right + origin_x.round() as i32,
        bottom: local.bottom + origin_y.round() as i32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tree() -> FrameNode {
        parse_frame_tree(&json!({
            "frameTree": {
                "frame": {"id":"root","url":"https://shop.example/","name":""},
                "childFrames": [
                    {"frame":{"id":"checkout","parentId":"root","url":"https://shop.example/frame","name":"checkout"}},
                    {"frame":{"id":"pay-a","parentId":"root","url":"https://pay.example/a","name":"payment"}},
                    {"frame":{"id":"pay-b","parentId":"root","url":"https://pay.example/b","name":"payment"},
                     "childFrames":[{"frame":{"id":"nested","parentId":"pay-b","url":"https://pay.example/nested","name":"card"}}]}
                ]
            }
        }))
        .unwrap()
    }

    fn owners() -> Vec<FrameOwnerBox> {
        vec![
            FrameOwnerBox {
                frame_id: "checkout".into(),
                css: Some("iframe#checkout".into()),
                name: "checkout".into(),
                url: "https://shop.example/frame".into(),
                left: 10.0,
                top: 20.0,
                right: 110.0,
                bottom: 80.0,
                unsupported_transform: false,
            },
            FrameOwnerBox {
                frame_id: "pay-a".into(),
                css: None,
                name: "payment".into(),
                url: "https://pay.example/a".into(),
                left: 0.0,
                top: 100.0,
                right: 50.0,
                bottom: 150.0,
                unsupported_transform: false,
            },
            FrameOwnerBox {
                frame_id: "pay-b".into(),
                css: None,
                name: "payment".into(),
                url: "https://pay.example/b".into(),
                left: 60.0,
                top: 100.0,
                right: 120.0,
                bottom: 160.0,
                unsupported_transform: false,
            },
            FrameOwnerBox {
                frame_id: "nested".into(),
                css: Some("iframe[name=\"card\"]".into()),
                name: "card".into(),
                url: "https://pay.example/nested".into(),
                left: 4.0,
                top: 6.0,
                right: 40.0,
                bottom: 30.0,
                unsupported_transform: false,
            },
        ]
    }

    #[test]
    fn cross_origin_target_must_belong_to_the_owned_frame_tree() {
        let owned = frame_ids(&tree());
        let page = "page-session";
        let attached = vec![
            AttachedTarget {
                session_id: "child".into(),
                parent_session_id: Some(page.into()),
                target_type: "iframe".into(),
                root_frame_id: "pay-b".into(),
            },
            AttachedTarget {
                session_id: "stranger".into(),
                parent_session_id: Some(page.into()),
                target_type: "iframe".into(),
                root_frame_id: "other-page".into(),
            },
            AttachedTarget {
                session_id: "nested".into(),
                parent_session_id: Some("child".into()),
                target_type: "iframe".into(),
                root_frame_id: "nested".into(),
            },
            AttachedTarget {
                session_id: "worker".into(),
                parent_session_id: Some(page.into()),
                target_type: "service_worker".into(),
                root_frame_id: "pay-b".into(),
            },
        ];
        let authorized = authorize_frames(page, &owned, &attached);
        assert_eq!(
            authorized
                .iter()
                .map(|frame| frame.frame_id.as_str())
                .collect::<Vec<_>>(),
            vec!["pay-b", "nested"]
        );
    }

    #[test]
    fn duplicate_frame_names_require_an_explicit_position() {
        let root = tree();
        let owners = owners();
        let ambiguous = WebContext {
            frame_path: vec![WebFrameSelector {
                name: Some("payment".into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(
            resolve_frame(&root, &owners, &ambiguous)
                .unwrap_err()
                .to_string()
                .contains("position")
        );
        let second = WebContext {
            frame_path: vec![WebFrameSelector {
                name: Some("payment".into()),
                position: Some(2),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(resolve_frame(&root, &owners, &second).unwrap().id, "pay-b");
    }

    #[test]
    fn nested_bounds_add_owner_offsets_and_reject_rotation() {
        let root = tree();
        let mut owners = owners();
        let context = WebContext {
            frame_path: vec![
                WebFrameSelector {
                    name: Some("payment".into()),
                    position: Some(2),
                    ..Default::default()
                },
                WebFrameSelector {
                    css: Some("iframe[name=\"card\"]".into()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let frame = resolve_frame(&root, &owners, &context).unwrap();
        let mut chain = Vec::new();
        fn collect<'a>(node: &'a FrameNode, id: &str, chain: &mut Vec<&'a FrameNode>) -> bool {
            if node.id == id {
                return true;
            }
            for child in &node.children {
                chain.push(child);
                if collect(child, id, chain) {
                    return true;
                }
                chain.pop();
            }
            false
        }
        assert!(collect(&root, &frame.id, &mut chain));
        let mapped = map_bounds(
            Bounds {
                left: 1,
                top: 2,
                right: 3,
                bottom: 4,
            },
            &owners,
            &chain,
        )
        .unwrap();
        assert_eq!(
            mapped,
            Bounds {
                left: 65,
                top: 108,
                right: 67,
                bottom: 110
            }
        );
        owners[3].unsupported_transform = true;
        assert!(
            map_bounds(
                Bounds {
                    left: 1,
                    top: 2,
                    right: 3,
                    bottom: 4
                },
                &owners,
                &chain
            )
            .is_err()
        );
    }

    #[test]
    fn a_replaced_frame_does_not_fall_back_to_the_root() {
        let root = tree();
        let missing = WebContext {
            frame_path: vec![WebFrameSelector {
                css: Some("iframe#gone".into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let error = resolve_frame(&root, &owners(), &missing)
            .unwrap_err()
            .to_string();
        assert!(error.contains("not found"), "{error}");
        assert!(!error.contains("root"));
    }
}
