//! Resolves inputs and expands validated YAML steps.

use super::{
    FlowDocument, FlowWebContext, Profile, Selector, Step,
    actions::{network_fault_step, pointer_gesture_step, swipe_element_step, validate_action_keys},
};
use crate::mcp::Result;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub fn resolve(value: &Value, variables: &BTreeMap<String, Value>) -> Result<Value> {
    resolve_scoped(value, variables, None)
}

fn resolve_scoped(
    value: &Value,
    variables: &BTreeMap<String, Value>,
    environment: Option<&BTreeMap<String, String>>,
) -> Result<Value> {
    Ok(match value {
        Value::String(text) => {
            if text.starts_with("${") && text.ends_with('}') && text.matches("${").count() == 1 {
                let key = &text[2..text.len() - 1];
                variable(key, variables, environment)?
            } else {
                let mut rest = text.as_str();
                let mut result = String::new();
                while let Some(start) = rest.find("${") {
                    // Exporters escape literal template openers by doubling `$`.
                    // Consume only the opener: even `${unfinished` is valid input
                    // when it came from a recording rather than a template.
                    if let Some(prefix) = rest[..start].strip_suffix('$') {
                        result.push_str(prefix);
                        result.push_str("${");
                        rest = &rest[start + 2..];
                        continue;
                    }
                    result.push_str(&rest[..start]);
                    let end =
                        rest[start + 2..].find('}').ok_or("unterminated variable")? + start + 2;
                    let v = variable(&rest[start + 2..end], variables, environment)?;
                    result.push_str(v.as_str().ok_or("embedded variable must be a string")?);
                    rest = &rest[end + 1..];
                }
                result.push_str(rest);
                Value::String(result)
            }
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|v| resolve_scoped(v, variables, environment))
                .collect::<Result<_>>()?,
        ),
        Value::Object(items) => Value::Object(
            items
                .iter()
                .map(|(k, v)| Ok((k.clone(), resolve_scoped(v, variables, environment)?)))
                .collect::<Result<_>>()?,
        ),
        value => value.clone(),
    })
}

fn variable(
    key: &str,
    variables: &BTreeMap<String, Value>,
    environment: Option<&BTreeMap<String, String>>,
) -> Result<Value> {
    if let Some(name) = key.strip_prefix("env:") {
        return environment_value(name, environment).map(Value::String);
    }
    variables
        .get(key)
        .cloned()
        .ok_or_else(|| crate::FlowError::from(format!("unresolved variable {key}")))
}

fn environment_value(name: &str, environment: Option<&BTreeMap<String, String>>) -> Result<String> {
    match environment {
        Some(values) => values.get(name).cloned(),
        None => std::env::var(name).ok(),
    }
    .ok_or_else(|| format!("missing environment variable {name}").into())
}

/// Expand declarative recipes before connecting to MCP. Recursive/unknown
/// recipes and unresolved selectors fail before the first device action.
pub fn compile(flow: &FlowDocument, profile: &Profile) -> Result<Vec<Step>> {
    compile_with_profile(flow, profile).map(|(steps, _)| steps)
}

pub(crate) fn compile_with_profile(
    flow: &FlowDocument,
    profile: &Profile,
) -> Result<(Vec<Step>, Profile)> {
    compile_with_environment(flow, profile, None)
}

/// Compile against an explicit environment, without modifying process globals.
pub fn compile_with_environment(
    flow: &FlowDocument,
    profile: &Profile,
    environment: Option<&BTreeMap<String, String>>,
) -> Result<(Vec<Step>, Profile)> {
    if profile
        .stable_samples
        .is_some_and(|n| !(1..=4).contains(&n))
    {
        return Err("profile stable_samples must be 1..4".into());
    }
    let mut vars = profile.variables.clone();
    vars.insert("app_id".into(), json!(profile.app_id));
    vars.insert("device".into(), profile.device.clone());
    // Resolve definitions once. Environment/recipe parameter values are opaque
    // data, so a password containing `${...}` is never interpolated a second time.
    let definitions = vars;
    let mut vars = BTreeMap::new();
    for name in definitions.keys() {
        vars.insert(
            name.clone(),
            resolve_definition(name, &definitions, &mut Vec::new(), environment)?,
        );
    }
    // Resolve selector templates once with the same opaque variable values as
    // step templates. Runtime lookups, including row/viewport relations, use
    // this resolved profile rather than interpolating already resolved data.
    let selector_source = serde_json::to_value(&profile.selectors).unwrap();
    let mut resolved_profile = profile.clone();
    resolved_profile.variables = vars.clone();
    resolved_profile.device = vars["device"].clone();
    resolved_profile.app_id = vars["app_id"].as_str().unwrap_or_default().to_owned();
    resolved_profile.selectors =
        serde_json::from_value(resolve_scoped(&selector_source, &vars, environment)?)
            .map_err(|e| format!("invalid resolved selectors: {e}"))?;
    let profile = &resolved_profile;
    for (name, selector) in &profile.selectors {
        validate_selector(name, selector, profile, &mut Vec::new())?;
    }
    for name in profile.recipes.keys() {
        check_recipe_cycles(name, profile, &mut Vec::new())?;
    }
    let steps = expand(
        &flow.steps,
        profile,
        &vars,
        &mut Vec::new(),
        "",
        environment,
    )?;
    Ok((steps, resolved_profile))
}

fn resolve_definition(
    name: &str,
    definitions: &BTreeMap<String, Value>,
    stack: &mut Vec<String>,
    environment: Option<&BTreeMap<String, String>>,
) -> Result<Value> {
    if stack.iter().any(|n| n == name) || stack.len() >= 16 {
        return Err(format!("cyclic or excessive variable nesting: {name}").into());
    }
    let value = definitions
        .get(name)
        .ok_or_else(|| format!("unresolved variable {name}"))?;
    fn references(value: &Value, found: &mut Vec<String>) -> Result<()> {
        match value {
            Value::String(text) => {
                let mut rest = text.as_str();
                while let Some(start) = rest.find("${") {
                    if rest[..start].ends_with('$') {
                        rest = &rest[start + 2..];
                        continue;
                    }
                    let end =
                        rest[start + 2..].find('}').ok_or("unterminated variable")? + start + 2;
                    let key = &rest[start + 2..end];
                    if !key.starts_with("env:") {
                        found.push(key.into());
                    }
                    rest = &rest[end + 1..];
                }
            }
            Value::Array(items) => {
                for v in items {
                    references(v, found)?;
                }
            }
            Value::Object(items) => {
                for v in items.values() {
                    references(v, found)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    let mut refs = Vec::new();
    references(value, &mut refs)?;
    stack.push(name.into());
    let mut resolved = BTreeMap::new();
    for key in refs {
        resolved.insert(
            key.clone(),
            resolve_definition(&key, definitions, stack, environment)?,
        );
    }
    stack.pop();
    resolve_scoped(value, &resolved, environment)
}

fn check_recipe_cycles(name: &str, profile: &Profile, stack: &mut Vec<String>) -> Result<()> {
    if stack.iter().any(|n| n == name) {
        return Err(format!("recipe cycle: {name}").into());
    }
    if stack.len() >= 16 {
        return Err("recipe nesting exceeds 16".into());
    }
    let steps = profile
        .recipes
        .get(name)
        .ok_or_else(|| format!("unknown recipe {name}"))?;
    stack.push(name.into());
    fn walk(steps: &[Step], p: &Profile, stack: &mut Vec<String>) -> Result<()> {
        for step in steps {
            if let Some(name) = step
                .action
                .get("run")
                .and_then(|v| v.get("recipe"))
                .and_then(Value::as_str)
            {
                check_recipe_cycles(name, p, stack)?;
            }
            if let Some(cond) = step.action.get("if_element") {
                for branch in ["then", "else"] {
                    let children: Vec<Step> =
                        serde_json::from_value(cond.get(branch).cloned().unwrap_or(json!([])))
                            .map_err(|e| e.to_string())?;
                    walk(&children, p, stack)?;
                }
            }
        }
        Ok(())
    }
    walk(steps, profile, stack)?;
    stack.pop();
    Ok(())
}

fn validate_selector(name: &str, s: &Selector, p: &Profile, stack: &mut Vec<String>) -> Result<()> {
    if let Some(position) = &s.position {
        position_index(position, 0)?;
        if s.native_tap {
            return Err("position cannot be combined with native_tap; use coordinate tap to preserve the selected match".into());
        }
    }
    if s.source
        .as_deref()
        .is_some_and(|v| !["auto", "native", "flutter"].contains(&v))
    {
        return Err("source must be auto, native, or flutter".into());
    }
    if stack.iter().any(|v| v == name) {
        return Err(format!("selector relation cycle: {name}").into());
    }
    if s.max_results.is_some_and(|n| !(1..=100).contains(&n)) {
        return Err("max_results must be 1..100".into());
    }
    if let Some(context) = &s.web_context {
        context.validate()?;
        if s.source.as_deref() == Some("flutter") && !context.is_empty() {
            return Err("web_context cannot be combined with source flutter".into());
        }
    }
    if s.stable_samples.is_some_and(|n| !(1..=4).contains(&n)) {
        return Err("stable_samples must be 1..4".into());
    }
    if let Some(t) = &s.element_type {
        if ![
            "textField",
            "button",
            "link",
            "staticText",
            "checkBox",
            "switch",
            "image",
            "radio",
            "slider",
        ]
        .contains(&t.as_str())
        {
            return Err(format!("unknown element_type {t}").into());
        }
    }
    for r in [&s.replace_recipe, &s.verify_recipe].into_iter().flatten() {
        if !p.recipes.contains_key(r) {
            return Err(format!("unknown field recipe {r}").into());
        }
    }
    stack.push(name.into());
    for target in [&s.row_with, &s.exclude_row_with].into_iter().flatten() {
        let related = p
            .selectors
            .get(target)
            .ok_or_else(|| crate::FlowError::from(format!("unknown selector {target}")))?;
        validate_selector(target, related, p, stack)?;
    }
    stack.pop();
    Ok(())
}

/// Resolve against the complete filtered snapshot; out-of-range positions do
/// not match. Keep tree order so inspector positions and execution agree.
pub fn position_index(position: &Value, len: usize) -> Result<Option<usize>> {
    let index = match position.as_str() {
        Some("first") => 0,
        Some("last") => return Ok(len.checked_sub(1)),
        _ => position
            .as_u64()
            .filter(|n| *n > 0)
            .and_then(|n| usize::try_from(n - 1).ok())
            .ok_or("position must be first, last, or a positive one-based integer")?,
    };
    Ok((index < len).then_some(index))
}

pub fn selector(args: &Value, profile: &Profile) -> Result<Selector> {
    if let Some(target) = args.get("target").and_then(Value::as_str) {
        return profile
            .selectors
            .get(target)
            .cloned()
            .ok_or_else(|| crate::FlowError::from(format!("unknown selector {target}")));
    }
    let mut raw = args.clone();
    if let Some(map) = raw.as_object_mut() {
        for key in [
            "expected",
            "count",
            "property",
            "timeout_ms",
            "samples",
            "interval_ms",
            "text",
            "text_from_env",
            "replacement",
            "direction",
            "from",
            "to",
            "duration_ms",
            "wheel_delta",
        ] {
            map.remove(key);
        }
    }
    let parsed: Selector =
        serde_json::from_value(raw).map_err(|e| format!("invalid selector: {e}"))?;
    if let Some(context) = &parsed.web_context {
        context.validate()?;
    }
    Ok(parsed)
}

fn expand(
    steps: &[Step],
    p: &Profile,
    vars: &BTreeMap<String, Value>,
    stack: &mut Vec<String>,
    prefix: &str,
    environment: Option<&BTreeMap<String, String>>,
) -> Result<Vec<Step>> {
    if stack.len() > 16 {
        return Err("recipe nesting exceeds 16".into());
    }
    let mut result = Vec::new();
    let mut ids = BTreeSet::new();
    for (index, step) in steps.iter().enumerate() {
        let id = format!(
            "{prefix}{}",
            step.id
                .clone()
                .unwrap_or_else(|| format!("step-{}", index + 1))
        );
        if !ids.insert(id.clone()) {
            return Err(format!("duplicate step ID {id}").into());
        }
        if step.action.len() != 1 {
            return Err(format!("{id}: exactly one action required").into());
        }
        let platform = step
            .platform
            .as_ref()
            .map(|value| {
                let resolved = resolve_scoped(&json!(value), vars, environment)?;
                let platform = resolved.as_str().ok_or("step platform must be a string")?;
                if !["android", "ios", "macos", "windows", "chrome"].contains(&platform) {
                    return Err(crate::FlowError::from(format!(
                        "{id}: unknown step platform {platform}"
                    )));
                }
                Ok(platform.to_owned())
            })
            .transpose()?;
        let resource = step
            .resource
            .as_ref()
            .map(|value| {
                let resolved = resolve_scoped(&json!(value), vars, environment)?;
                let name = resolved.as_str().ok_or("step resource must be a string")?;
                if name.trim().is_empty() || name.trim() != name {
                    return Err(crate::FlowError::from(format!(
                        "{id}: step resource must be a nonempty stable name"
                    )));
                }
                Ok(name.to_owned())
            })
            .transpose()?;
        if platform.is_some() && resource.is_some() {
            return Err(format!("{id}: step cannot specify both platform and resource").into());
        }
        let (action, raw) = step.action.iter().next().unwrap();
        if action == "select_device" && (platform.is_some() || resource.is_some()) {
            return Err(format!(
                "{id}: use select_device.platform for persistent device selection"
            )
            .into());
        }
        // Branch bodies are templates in the current recipe scope. Resolve each
        // leaf once during recursive expansion, never the already resolved data.
        let mut template = raw.clone();
        if action == "if_element" {
            if let Some(object) = template.as_object_mut() {
                object.remove("then");
                object.remove("else");
            }
        }
        let mut args = resolve_scoped(&template, vars, environment)?;
        validate_action_keys(action, &args)?;
        if let Some(value) = args.get("web_context") {
            let context: FlowWebContext = serde_json::from_value(value.clone())
                .map_err(|error| format!("invalid web_context: {error}"))?;
            context.validate()?;
        }
        if action == "run" {
            let name = args["recipe"]
                .as_str()
                .ok_or("run.recipe missing")?
                .to_owned();
            if stack.contains(&name) {
                return Err(format!("recipe cycle: {name}").into());
            }
            let recipe = p
                .recipes
                .get(&name)
                .ok_or_else(|| format!("unknown recipe {name}"))?;
            let mut local = vars.clone();
            if let Some(parameters) = args.get("with").and_then(Value::as_object) {
                local.extend(parameters.iter().map(|(k, v)| (k.clone(), v.clone())));
            }
            stack.push(name);
            let mut expanded = expand(recipe, p, &local, stack, &format!("{id}."), environment)?;
            for child in &mut expanded {
                if child.note.is_none() {
                    child.note = step.note.clone();
                }
                if child.platform.is_none() && child.resource.is_none() {
                    child.platform = platform.clone();
                    child.resource = resource.clone();
                }
            }
            result.extend(expanded);
            stack.pop();
            continue;
        }
        if action == "if_element" {
            selector(&json!({"target":args["target"]}), p)?;
            for branch in ["then", "else"] {
                let children: Vec<Step> =
                    serde_json::from_value(raw.get(branch).cloned().unwrap_or(json!([])))
                        .map_err(|e| e.to_string())?;
                args[branch] = serde_json::to_value(expand(
                    &children,
                    p,
                    vars,
                    stack,
                    &format!("{id}.{branch}."),
                    environment,
                )?)
                .unwrap();
            }
        }
        if [
            "tap_element",
            "long_press_element",
            "double_tap_element",
            "swipe_element",
            "assert_element",
            "assert_absent",
            "assert_state",
            "assert_value",
            "assert_count",
            "fill_field",
            "gesture",
            "scroll",
            "list_elements",
            "focus_field",
        ]
        .contains(&action.as_str())
        {
            let s = selector(&args, p)?;
            validate_selector(&id, &s, p, &mut Vec::new())?;
            if action == "fill_field" {
                if let Some(env) = args.get("text_from_env").and_then(Value::as_str) {
                    args["text"] = json!(environment_value(env, environment)?);
                }
                if args["text"].as_str().is_none_or(str::is_empty) {
                    return Err(format!("{id}: fill_field.text must not be empty").into());
                }
                for (field, recipe) in [
                    ("replacement", s.replace_recipe),
                    ("verification", s.verify_recipe),
                ] {
                    if let Some(recipe) = recipe {
                        if stack.contains(&recipe) {
                            return Err("replacement recipe cycle".into());
                        }
                        let mut local = vars.clone();
                        local.insert(
                            "target".into(),
                            args.get("target")
                                .cloned()
                                .ok_or("replacement requires logical target")?,
                        );
                        local.insert("text".into(), args["text"].clone());
                        stack.push(recipe.clone());
                        args[field] = serde_json::to_value(expand(
                            &p.recipes[&recipe],
                            p,
                            &local,
                            stack,
                            &format!("{id}.{field}."),
                            environment,
                        )?)
                        .unwrap();
                        stack.pop();
                    }
                }
            }
        }
        let allowed = [
            "select_device",
            "wait",
            "launch_app",
            "start_screen_recording",
            "stop_screen_recording",
            "screenshot",
            "tap",
            "tap_element",
            "long_press_element",
            "double_tap_element",
            "swipe_element",
            "type_text",
            "swipe",
            "assert_element",
            "list_elements",
            "fill_field",
            "focus_field",
            "assert_absent",
            "assert_state",
            "assert_value",
            "assert_count",
            "gesture",
            "scroll",
            "if_element",
            "reset_fields",
            "note_limitation",
            "network_fault",
            "call_tool",
        ];
        if !allowed.contains(&action.as_str()) {
            return Err(format!("unknown action {action}").into());
        }
        if action == "wait" && args["ms"].as_u64().is_none_or(|n| n > 600_000) {
            return Err("wait.ms must be 0..600000".into());
        }
        if action == "list_elements" {
            if let Some(samples) = args.get("samples")
                && samples.as_u64().is_none_or(|n| !(1..=20).contains(&n))
            {
                return Err("list_elements.samples must be 1..20".into());
            }
            if let Some(interval) = args.get("interval_ms")
                && (args["samples"].as_u64().unwrap_or(1) < 2
                    || interval.as_u64().is_none_or(|n| !(50..=2_000).contains(&n)))
            {
                return Err(
                    "list_elements.interval_ms must be 50..2000 with at least two samples".into(),
                );
            }
        }
        if action == "network_fault" {
            network_fault_step(&args)?;
        }
        if matches!(action.as_str(), "long_press_element" | "double_tap_element") {
            pointer_gesture_step(action, &args)?;
        }
        if action == "swipe_element" {
            swipe_element_step(&args)?;
        }
        if let Some(n) = args.get("timeout_ms") {
            if n.as_u64().is_none_or(|n| n == 0 || n > 120_000) {
                return Err("timeout_ms must be 1..120000".into());
            }
        }
        result.push(Step {
            note: step.note.clone(),
            platform,
            resource,
            id: Some(id),
            action: BTreeMap::from([(action.clone(), args)]),
        });
    }
    if result.len() > 10_000 {
        return Err("expanded scenario exceeds 10000 steps".into());
    }
    Ok(result)
}
