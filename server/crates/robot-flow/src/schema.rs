use crate::mcp::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(flatten)]
    pub action: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Selector {
    /// One-based position among matches, or "first" / "last".
    #[serde(default)]
    pub position: Option<Value>,
    #[serde(default)]
    pub source: Option<String>,
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
    pub scroll_into_view: bool,
    #[serde(default)]
    pub max_results: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web_context: Option<FlowWebContext>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FlowWebFrame {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub css: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<u32>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FlowWebPage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Stable frame identity. YAML uses snake_case; runtime ids are rejected.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FlowWebContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<FlowWebPage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frame_path: Vec<FlowWebFrame>,
}

impl FlowWebContext {
    pub fn is_empty(&self) -> bool {
        self.app_id.as_deref().unwrap_or("").is_empty()
            && self.page.as_ref().is_none_or(|page| {
                page.url.as_deref().unwrap_or("").is_empty()
                    && page.title.as_deref().unwrap_or("").is_empty()
            })
            && self.frame_path.is_empty()
    }

    pub fn validate(&self) -> Result<()> {
        for (index, frame) in self.frame_path.iter().enumerate() {
            let css = frame.css.as_deref().unwrap_or("").trim();
            let name = frame.name.as_deref().unwrap_or("").trim();
            let url = frame.url.as_deref().unwrap_or("").trim();
            if css.is_empty() && name.is_empty() && url.is_empty() {
                return Err(
                    format!("web frame {} needs a css selector, name, or url", index + 1).into(),
                );
            }
            if frame.position.is_some_and(|position| position == 0) {
                return Err("web frame position is one-based".into());
            }
        }
        Ok(())
    }

    pub fn to_tool_value(&self) -> Value {
        json!({
            "appId": self.app_id,
            "page": self.page.as_ref().map(|page| json!({"url": page.url, "title": page.title})),
            "framePath": self.frame_path.iter().map(|frame| json!({
                "css": frame.css,
                "name": frame.name,
                "url": frame.url,
                "position": frame.position,
            })).collect::<Vec<_>>(),
        })
    }
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

mod actions;
mod compiler;

pub(crate) use actions::{
    NetworkFaultStep, network_fault_step, pointer_gesture_step, scroll_wheel_delta,
    swipe_element_step,
};
pub use compiler::{compile, compile_with_environment, position_index, resolve, selector};

#[cfg(test)]
mod input_metadata_tests {
    use super::*;

    #[test]
    fn intent_notes_are_metadata_and_inherit_through_recipes() {
        let flow = FlowDocument::from_yaml("version: 2\nname: notes\nsteps:\n- id: prepare\n  note: Mở form đăng nhập\n  run: {recipe: ready}\n").unwrap();
        let mut profile = Profile::default();
        profile.recipes.insert(
            "ready".into(),
            serde_yaml::from_str(
                "- wait: {ms: 1}\n- note: Chờ giao diện ổn định\n  wait: {ms: 2}\n",
            )
            .unwrap(),
        );
        let steps = compile(&flow, &profile).unwrap();
        assert_eq!(steps[0].note.as_deref(), Some("Mở form đăng nhập"));
        assert_eq!(steps[1].note.as_deref(), Some("Chờ giao diện ổn định"));
        assert!(
            steps
                .iter()
                .all(|step| step.action.len() == 1 && !step.action.contains_key("note"))
        );
    }

    #[test]
    fn scoped_environment_resolves_nested_templates_and_text_without_process_fallback() {
        let flow = FlowDocument::from_yaml(
            r#"
version: 2
name: scoped
steps:
  - run: {recipe: input}
"#,
        )
        .unwrap();
        let mut profile = Profile::default();
        profile
            .variables
            .insert("secret".into(), json!("${env:SCOPED_SECRET}"));
        profile.recipes.insert(
            "input".into(),
            serde_yaml::from_str(
                r#"
- type_text: {text: "${secret}"}
- fill_field: {filter: field, text_from_env: SCOPED_SECRET}
"#,
            )
            .unwrap(),
        );
        let env = BTreeMap::from([("SCOPED_SECRET".into(), "opaque-${literal}".into())]);
        let (steps, _) = compile_with_environment(&flow, &profile, Some(&env)).unwrap();
        assert_eq!(steps[0].action["type_text"]["text"], "opaque-${literal}");
        assert_eq!(steps[1].action["fill_field"]["text"], "opaque-${literal}");
        assert!(compile_with_environment(&flow, &profile, Some(&BTreeMap::new())).is_err());
    }

    #[test]
    fn environment_configuration_does_not_inject_sensitive_flags() {
        let flow: FlowDocument = serde_json::from_value(json!({
            "version":2,"name":"boot and input","steps":[
                {"call_tool":{"name":"mobile_boot_device","arguments":{"deviceId":"chrome:browser"}}},
                {"type_text":{"text":"fixture"}},
                {"call_tool":{"name":"mobile_type_keys","arguments":{"deviceId":"fixture","text":"fixture"}}},
                {"call_tool":{"name":"mobile_boot_device","arguments":{"deviceId":"chrome:browser"},"sensitive":true}}
            ]
        })).unwrap();
        let mut profile = Profile::default();
        profile
            .variables
            .insert("environment_setting".into(), json!("${env:PATH}"));
        let steps = compile(&flow, &profile).unwrap();
        assert_ne!(
            steps[0].action["call_tool"].get("sensitive"),
            Some(&json!(true))
        );
        assert!(steps[1].action["type_text"].get("sensitive").is_none());
        assert!(steps[2].action["call_tool"].get("sensitive").is_none());
        assert_eq!(steps[3].action["call_tool"]["sensitive"], true);
    }

    #[test]
    fn web_context_keeps_stable_frame_selectors() {
        let rejected = compile(
            &FlowDocument::from_yaml(
                "version: 2\nname: frame\nsteps:\n- tap_element:\n    web_context:\n      frame_path:\n        - sessionId: abc\n",
            )
            .unwrap(),
            &Profile::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(rejected.contains("sessionId"), "{rejected}");
        let empty = compile(
            &FlowDocument::from_yaml(
                "version: 2\nname: frame\nsteps:\n- tap_element:\n    web_context:\n      frame_path:\n        - position: 1\n",
            )
            .unwrap(),
            &Profile::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(empty.contains("css selector"), "{empty}");
        compile(
            &FlowDocument::from_yaml(
                "version: 2\nname: frame\nsteps:\n- tap_element:\n    web_context:\n      frame_path:\n        - css: \"iframe#checkout\"\n          name: payment\n    resource_id: submit\n- type_text:\n    text: paid\n    web_context:\n      frame_path:\n        - name: payment\n",
            )
            .unwrap(),
            &Profile::default(),
        )
        .unwrap();
    }
}

#[cfg(test)]
mod literal_escape_tests {
    use super::*;

    #[test]
    fn escaped_templates_are_literal_even_when_unclosed_or_nested() {
        for (source, expected) in [
            ("$${missing}", "${missing}"),
            ("$$${dollars}", "$${dollars}"),
            ("$${unfinished", "${unfinished"),
            ("$${outer$${inner}}", "${outer${inner}}"),
        ] {
            assert_eq!(
                resolve(&json!(source), &BTreeMap::new()).unwrap(),
                json!(expected)
            );
        }
    }

    #[test]
    fn literal_text_and_real_variables_can_coexist_without_recursive_expansion() {
        let variables = BTreeMap::from([
            ("name".into(), json!("${opaque}")),
            ("list".into(), json!([1, 2])),
        ]);
        assert_eq!(
            resolve(
                &json!({"text":"$${missing} / ${name}", "items":"${list}"}),
                &variables
            )
            .unwrap(),
            json!({"text":"${missing} / ${opaque}", "items":[1, 2]})
        );
    }

    #[test]
    fn escaped_profile_values_do_not_create_phantom_dependencies() {
        let mut profile = Profile::default();
        profile
            .variables
            .insert("recorded".into(), json!("$${missing} / $${unfinished"));
        let flow = FlowDocument::from_yaml(
            "version: 2\nname: literal\nsteps:\n  - type_text: {text: '${recorded}'}\n",
        )
        .unwrap();
        let steps = compile(&flow, &profile).unwrap();
        assert_eq!(
            steps[0].action["type_text"]["text"],
            "${missing} / ${unfinished"
        );
    }
}
