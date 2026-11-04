use crate::mcp::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FlowDocument {
    pub version: u32,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub preconditions: Vec<String>,
    pub steps: Vec<Step>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Step {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(flatten)]
    pub action: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Selector {
    #[serde(default)]
    pub filter: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub element_type: Option<String>,
    #[serde(default)]
    pub resource_id: Option<String>,
    #[serde(default)]
    pub prefer_resource_id: Option<String>,
    #[serde(default)]
    pub class_contains: Option<String>,
    #[serde(default)]
    pub row_with: Option<String>,
    #[serde(default)]
    pub exclude_row_with: Option<String>,
    #[serde(default)]
    pub allow_missing_row_anchor: bool,
    #[serde(default)]
    pub largest: bool,
    #[serde(default)]
    pub native_tap: bool,
    #[serde(default)]
    pub stable_samples: Option<usize>,
    #[serde(default)]
    pub replace_recipe: Option<String>,
    #[serde(default)]
    pub verify_recipe: Option<String>,
    #[serde(default)]
    pub require_focus: bool,
    #[serde(default)]
    pub max_results: Option<usize>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    #[serde(default)]
    pub device: Value,
    #[serde(default)]
    pub app_id: String,
    #[serde(default)]
    pub variables: BTreeMap<String, Value>,
    #[serde(default)]
    pub selectors: BTreeMap<String, Selector>,
    #[serde(default)]
    pub recipes: BTreeMap<String, Vec<Step>>,
    #[serde(default)]
    pub viewport: Option<String>,
    #[serde(default)]
    pub preconditions: Vec<String>,
    #[serde(default)]
    pub stable_samples: Option<usize>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profiles {
    pub version: u32,
    #[serde(default)]
    pub selectors: BTreeMap<String, Selector>,
    #[serde(default)]
    pub recipes: BTreeMap<String, Vec<Step>>,
    pub targets: BTreeMap<String, Profile>,
}

impl Profiles {
    pub fn from_path(path: &Path) -> Result<Self> {
        let mut yaml: serde_yaml::Value =
            serde_yaml::from_str(&fs::read_to_string(path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        yaml.apply_merge().map_err(|e| e.to_string())?;
        let mut profiles: Self = serde_yaml::from_value(yaml).map_err(|e| e.to_string())?;
        if profiles.version != 1 {
            return Err("unsupported profiles version".into());
        }
        for target in profiles.targets.values_mut() {
            let mut selectors = profiles.selectors.clone();
            selectors.append(&mut target.selectors);
            target.selectors = selectors;
            let mut recipes = profiles.recipes.clone();
            recipes.append(&mut target.recipes);
            target.recipes = recipes;
        }
        Ok(profiles)
    }
}

impl FlowDocument {
    pub fn from_yaml(source: &str) -> Result<Self> {
        let flow: Self = serde_yaml::from_str(source).map_err(|e| e.to_string())?;
        if ![1, 2].contains(&flow.version) {
            return Err("unsupported scenario version (expected 1 or 2)".into());
        }
        if flow.name.trim().is_empty() || flow.steps.is_empty() {
            return Err("scenario needs name and steps".into());
        }
        Ok(flow)
    }
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_yaml(&fs::read_to_string(path).map_err(|e| e.to_string())?)
    }
}

pub fn resolve(value: &Value, variables: &BTreeMap<String, Value>) -> Result<Value> {
    Ok(match value {
        Value::String(text) => {
            if text.starts_with("${") && text.ends_with('}') && text.matches("${").count() == 1 {
                let key = &text[2..text.len() - 1];
                variable(key, variables)?
            } else {
                let mut rest = text.as_str();
                let mut result = String::new();
                while let Some(start) = rest.find("${") {
                    result.push_str(&rest[..start]);
                    let end =
                        rest[start + 2..].find('}').ok_or("unterminated variable")? + start + 2;
                    let v = variable(&rest[start + 2..end], variables)?;
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
                .map(|v| resolve(v, variables))
                .collect::<Result<_>>()?,
        ),
        Value::Object(items) => Value::Object(
            items
                .iter()
                .map(|(k, v)| Ok((k.clone(), resolve(v, variables)?)))
                .collect::<Result<_>>()?,
        ),
        value => value.clone(),
    })
}

fn variable(key: &str, variables: &BTreeMap<String, Value>) -> Result<Value> {
    if let Some(name) = key.strip_prefix("env:") {
        return std::env::var(name)
            .map(Value::String)
            .map_err(|_| format!("missing environment variable {name}"));
    }
    variables
        .get(key)
        .cloned()
        .ok_or_else(|| format!("unresolved variable {key}"))
}

/// Expand declarative recipes before connecting to MCP. Recursive/unknown
/// recipes and unresolved selectors fail before the first device action.
pub fn compile(flow: &FlowDocument, profile: &Profile) -> Result<Vec<Step>> {
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
            resolve_definition(name, &definitions, &mut Vec::new())?,
        );
    }
    let source = serde_json::to_string(&flow.steps).unwrap();
    let profile_source = serde_json::to_string(&profile.variables).unwrap();
    let recipe_source = serde_json::to_string(&profile.recipes).unwrap();
    vars.insert(
        "__sensitive_inputs".into(),
        json!(
            source.contains("${env:")
                || source.contains("text_from_env")
                || profile_source.contains("${env:")
                || recipe_source.contains("${env:")
                || recipe_source.contains("text_from_env")
        ),
    );
    for (name, selector) in &profile.selectors {
        validate_selector(name, selector, profile, &mut Vec::new())?;
    }
    for name in profile.recipes.keys() {
        check_recipe_cycles(name, profile, &mut Vec::new())?;
    }
    expand(&flow.steps, profile, &vars, &mut Vec::new(), "")
}

fn resolve_definition(
    name: &str,
    definitions: &BTreeMap<String, Value>,
    stack: &mut Vec<String>,
) -> Result<Value> {
    if stack.iter().any(|n| n == name) || stack.len() >= 16 {
        return Err(format!("cyclic or excessive variable nesting: {name}"));
    }
    let value = definitions
        .get(name)
        .ok_or_else(|| format!("unresolved variable {name}"))?;
    fn references(value: &Value, found: &mut Vec<String>) -> Result<()> {
        match value {
            Value::String(text) => {
                let mut rest = text.as_str();
                while let Some(start) = rest.find("${") {
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
        resolved.insert(key.clone(), resolve_definition(&key, definitions, stack)?);
    }
    stack.pop();
    resolve(value, &resolved)
}

fn check_recipe_cycles(name: &str, profile: &Profile, stack: &mut Vec<String>) -> Result<()> {
    if stack.iter().any(|n| n == name) {
        return Err(format!("recipe cycle: {name}"));
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
    if stack.iter().any(|v| v == name) {
        return Err(format!("selector relation cycle: {name}"));
    }
    if s.max_results.is_some_and(|n| !(1..=100).contains(&n)) {
        return Err("max_results must be 1..100".into());
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
            return Err(format!("unknown element_type {t}"));
        }
    }
    for r in [&s.replace_recipe, &s.verify_recipe].into_iter().flatten() {
        if !p.recipes.contains_key(r) {
            return Err(format!("unknown field recipe {r}"));
        }
    }
    stack.push(name.into());
    for target in [&s.row_with, &s.exclude_row_with].into_iter().flatten() {
        let related = p
            .selectors
            .get(target)
            .ok_or_else(|| format!("unknown selector {target}"))?;
        validate_selector(target, related, p, stack)?;
    }
    stack.pop();
    Ok(())
}

pub fn selector(args: &Value, profile: &Profile) -> Result<Selector> {
    if let Some(target) = args.get("target").and_then(Value::as_str) {
        return profile
            .selectors
            .get(target)
            .cloned()
            .ok_or_else(|| format!("unknown selector {target}"));
    }
    let mut raw = args.clone();
    if let Some(map) = raw.as_object_mut() {
        for key in [
            "expected",
            "count",
            "property",
            "timeout_ms",
            "text",
            "text_from_env",
            "replacement",
            "direction",
            "from",
            "to",
            "duration_ms",
        ] {
            map.remove(key);
        }
    }
    serde_json::from_value(raw).map_err(|e| format!("invalid selector: {e}"))
}

fn expand(
    steps: &[Step],
    p: &Profile,
    vars: &BTreeMap<String, Value>,
    stack: &mut Vec<String>,
    prefix: &str,
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
            return Err(format!("duplicate step ID {id}"));
        }
        if step.action.len() != 1 {
            return Err(format!("{id}: exactly one action required"));
        }
        let (action, raw) = step.action.iter().next().unwrap();
        // Branch bodies are templates in the current recipe scope. Resolve each
        // leaf once during recursive expansion, never the already resolved data.
        let mut template = raw.clone();
        if action == "if_element" {
            if let Some(object) = template.as_object_mut() {
                object.remove("then");
                object.remove("else");
            }
        }
        let mut args = resolve(&template, vars)?;
        validate_action_keys(action, &args)?;
        if ["fill_field", "type_text", "call_tool"].contains(&action.as_str())
            && (vars.get("__sensitive_inputs") == Some(&json!(true))
                || args.get("text_from_env").is_some()
                || args.get("sensitive") == Some(&json!(true)))
        {
            args["sensitive"] = json!(true);
        }
        if action == "run" {
            let name = args["recipe"]
                .as_str()
                .ok_or("run.recipe missing")?
                .to_owned();
            if stack.contains(&name) {
                return Err(format!("recipe cycle: {name}"));
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
            result.extend(expand(recipe, p, &local, stack, &format!("{id}."))?);
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
                )?)
                .unwrap();
            }
        }
        if [
            "tap_element",
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
                    args["text"] = json!(
                        std::env::var(env)
                            .map_err(|_| format!("missing environment variable {env}"))?
                    );
                }
                if args["text"].as_str().is_none_or(str::is_empty) {
                    return Err(format!("{id}: fill_field.text must not be empty"));
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
            "open_url",
            "start_screen_recording",
            "stop_screen_recording",
            "screenshot",
            "tap",
            "tap_element",
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
            "call_tool",
        ];
        if !allowed.contains(&action.as_str()) {
            return Err(format!("unknown action {action}"));
        }
        if action == "wait" && args["ms"].as_u64().is_none_or(|n| n > 600_000) {
            return Err("wait.ms must be 0..600000".into());
        }
        if let Some(n) = args.get("timeout_ms") {
            if n.as_u64().is_none_or(|n| n == 0 || n > 120_000) {
                return Err("timeout_ms must be 1..120000".into());
            }
        }
        result.push(Step {
            id: Some(id),
            action: BTreeMap::from([(action.clone(), args)]),
        });
    }
    if result.len() > 10_000 {
        return Err("expanded scenario exceeds 10000 steps".into());
    }
    Ok(result)
}

fn validate_action_keys(action: &str, args: &Value) -> Result<()> {
    let selector = "target filter label labels element_type resource_id prefer_resource_id class_contains row_with exclude_row_with allow_missing_row_anchor largest native_tap stable_samples replace_recipe verify_recipe require_focus max_results timeout_ms";
    let extra = match action {
        "run" => "recipe with",
        "if_element" => "target then else",
        "select_device" => "id name platform state",
        "launch_app" => "app_id",
        "open_url" => "url app_id",
        "wait" => "ms",
        "screenshot" => "path",
        "tap" => "x y",
        "type_text" => "text sensitive",
        "swipe" => "start end duration_ms",
        "call_tool" => "name arguments sensitive",
        "note_limitation" => "message",
        "start_screen_recording" | "stop_screen_recording" | "reset_fields" => "",
        "tap_element" | "assert_element" | "assert_absent" | "list_elements" | "focus_field" => {
            selector
        }
        "assert_state" => return check_keys(args, &format!("{selector} property expected")),
        "assert_value" => return check_keys(args, &format!("{selector} expected")),
        "assert_count" => return check_keys(args, &format!("{selector} count")),
        "fill_field" => {
            return check_keys(args, &format!("{selector} text text_from_env sensitive"));
        }
        "gesture" => return check_keys(args, &format!("{selector} from to duration_ms")),
        "scroll" => return check_keys(args, &format!("{selector} direction")),
        _ => return Err(format!("unknown action {action}")),
    };
    check_keys(args, extra)
}
fn check_keys(args: &Value, allowed: &str) -> Result<()> {
    let map = args
        .as_object()
        .ok_or("action arguments must be a mapping")?;
    for key in map.keys() {
        if !allowed.split_whitespace().any(|k| k == key) {
            return Err(format!("unknown action argument {key}"));
        }
    }
    Ok(())
}
