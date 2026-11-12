//! Group completed flows by the app or web origin they exercised.

use crate::model::CaseReport;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetRef {
    pub key: String,
    pub name: String,
}

pub fn unclassified() -> TargetRef {
    TargetRef {
        key: "unclassified".into(),
        name: "Chưa phân loại".into(),
    }
}

/// Declared metadata first, then YAML/report hints. Chrome/web URLs win over app IDs
/// when both exist so a browser flow is not filed under a desktop helper app.
pub fn classify(
    report: &CaseReport,
    app_id: Option<&str>,
    url: Option<&str>,
    yaml: Option<&str>,
) -> Vec<TargetRef> {
    let mut urls = Vec::new();
    push_url(&mut urls, url);
    if let Some(yaml) = yaml {
        for found in yaml_urls(yaml) {
            push_url(&mut urls, Some(&found));
        }
    }
    let mut apps = Vec::new();
    push_app(&mut apps, app_id);
    if let Some(yaml) = yaml {
        for found in yaml_app_ids(yaml) {
            push_app(&mut apps, Some(&found));
        }
    }
    let platform = report
        .selected_device
        .as_ref()
        .and_then(|device| device.get("platform"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let chrome = platform.is_some_and(|value| value.eq_ignore_ascii_case("chrome"));
    let mut targets = Vec::new();
    if chrome || !urls.is_empty() {
        for host in urls {
            push_unique(
                &mut targets,
                TargetRef {
                    key: format!("web:{host}"),
                    name: format!("Web · {host}"),
                },
            );
        }
    }
    if !chrome {
        if let Some(platform) = platform {
            if apps.is_empty() {
                push_unique(
                    &mut targets,
                    TargetRef {
                        key: platform.to_owned(),
                        name: platform_label(platform),
                    },
                );
            } else {
                for app in &apps {
                    push_unique(
                        &mut targets,
                        TargetRef {
                            key: format!("{platform}:{app}"),
                            name: format!("{} · {app}", platform_label(platform)),
                        },
                    );
                }
            }
        } else {
            for app in apps {
                push_unique(
                    &mut targets,
                    TargetRef {
                        key: format!("app:{app}"),
                        name: format!("App · {app}"),
                    },
                );
            }
        }
    }
    if targets.is_empty() {
        vec![unclassified()]
    } else {
        targets
    }
}

fn platform_label(platform: &str) -> String {
    match platform {
        "android" => "Android".into(),
        "ios" => "iOS".into(),
        "macos" => "macOS".into(),
        "windows" => "Windows".into(),
        "chrome" => "Web".into(),
        other => other.to_owned(),
    }
}

fn push_unique(targets: &mut Vec<TargetRef>, target: TargetRef) {
    if !target.key.contains('/')
        && !target.key.contains('\\')
        && !target.key.contains('\0')
        && !targets.iter().any(|existing| existing.key == target.key)
    {
        targets.push(target);
    }
}

fn push_url(urls: &mut Vec<String>, url: Option<&str>) {
    if let Some(host) = url.and_then(host_from_url) {
        if !urls.contains(&host) {
            urls.push(host);
        }
    }
}

fn push_app(apps: &mut Vec<String>, app_id: Option<&str>) {
    let Some(app) = app_id.map(str::trim).filter(|value| {
        !value.is_empty() && *value != "<selected>" && !value.contains('/') && !value.contains('\\')
    }) else {
        return;
    };
    let app = app.to_owned();
    if !apps.contains(&app) {
        apps.push(app);
    }
}

pub fn host_from_url(url: &str) -> Option<String> {
    let url = url
        .trim()
        .trim_matches(|c| matches!(c, '"' | '\'' | '<' | '>'));
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let host = rest
        .split(['/', '?', '#'])
        .next()?
        .rsplit('@')
        .next()?
        .split('%')
        .next()?
        .split(':')
        .next()?
        .trim()
        .trim_matches('.')
        .to_ascii_lowercase();
    if host.is_empty() || host == "localhost" || host.contains('/') {
        return None;
    }
    let ok = host
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'));
    ok.then_some(host)
}

fn yaml_urls(text: &str) -> Vec<String> {
    let mut urls = Vec::new();
    for prefix in ["https://", "http://"] {
        let mut from = 0;
        while let Some(at) = text[from..].find(prefix) {
            let start = from + at;
            let rest = &text[start..];
            let end = rest
                .find(|c: char| {
                    c.is_whitespace() || matches!(c, '"' | '\'' | ')' | ']' | ',' | '}' | '<' | '>')
                })
                .unwrap_or(rest.len());
            urls.push(rest[..end].to_owned());
            from = start + prefix.len();
        }
    }
    urls
}

fn yaml_app_ids(text: &str) -> Vec<String> {
    let mut apps = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        for prefix in ["app_id:", "appId:"] {
            if let Some(value) = trimmed.strip_prefix(prefix) {
                let value = value
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .trim()
                    .to_owned();
                if !value.is_empty() {
                    apps.push(value);
                }
            }
        }
    }
    let mut from = 0;
    while let Some(at) = text[from..].find("\"appId\"") {
        let rest = &text[from + at..];
        if let Some(colon) = rest.find(':') {
            let value = rest[colon + 1..]
                .trim()
                .trim_start_matches('"')
                .split('"')
                .next()
                .unwrap_or("")
                .trim();
            if !value.is_empty() {
                apps.push(value.to_owned());
            }
        }
        from += at + 7;
    }
    apps
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn report(platform: &str) -> CaseReport {
        CaseReport {
            name: "case".into(),
            status: "PASS".into(),
            selected_device: Some(json!({"platform": platform, "id": "dev"})),
            ..CaseReport::default()
        }
    }

    #[test]
    fn android_app_uses_platform_and_bundle() {
        let groups = classify(
            &report("android"),
            Some("com.example.orbit"),
            None,
            Some("  launch_app:\n    app_id: com.example.orbit\n"),
        );
        assert_eq!(groups[0].key, "android:com.example.orbit");
        assert!(groups[0].name.contains("Android"));
    }

    #[test]
    fn chrome_url_groups_by_origin() {
        let yaml = r#"call_tool: {name: mobile_open_url, arguments: {"url":"https://shop.example.com/login"}}"#;
        let groups = classify(&report("chrome"), None, None, Some(yaml));
        assert_eq!(groups[0].key, "web:shop.example.com");
    }

    #[test]
    fn missing_metadata_is_unclassified() {
        let groups = classify(
            &CaseReport {
                name: "case".into(),
                status: "FAIL".into(),
                dry_run: false,
                selected_device: None,
                steps: Vec::new(),
                limitations: Vec::new(),
                preconditions: Vec::new(),
                error: Some("setup".into()),
                error_kind: Some("setup".into()),
            },
            None,
            None,
            None,
        );
        assert_eq!(groups, vec![unclassified()]);
    }
}
