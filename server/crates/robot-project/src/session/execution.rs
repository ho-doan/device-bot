//! Prepares resources and runs cases in a project session.

use std::{collections::BTreeMap, path::Path, sync::Arc, time::Duration};

use robot_flow::{FlowRunner, mcp};
use serde_json::{Value, json};

use crate::{
    Project,
    plan::{Capability, HostPlatform, Preparation},
    recording::{self, SharedRecorder, StartPhase},
};

use super::{
    Case, Connected, Router, SessionFactory, SessionResult, Shared, capability_tools,
    discover_device, invoke, named_resource_devices, platform, proxy_configuration,
    sanitize_case_error,
};

pub(super) struct RunContext<'a> {
    pub(super) project: &'a Project,
    pub(super) factory: &'a dyn SessionFactory,
    pub(super) directory: &'a Path,
    pub(super) confirmed: &'a [String],
    pub(super) case_timeout: Duration,
    pub(super) history_root: Option<&'a Path>,
}

pub(super) async fn run_owned(
    context: RunContext<'_>,
    environment: BTreeMap<String, String>,
    cases: Vec<Case>,
    sessions: Shared,
    recorder: Option<SharedRecorder>,
) -> mcp::Result<SessionResult> {
    let RunContext {
        project,
        factory,
        directory,
        confirmed,
        case_timeout,
        history_root,
    } = context;
    let mut devices = BTreeMap::new();
    let mut common_tools: Option<BTreeMap<String, Value>> = None;
    let configs = project
        .plan
        .resources
        .iter()
        .map(|resource| {
            project
                .resolve_web_proxy(resource)
                .map(|config| (resource.name.clone(), config))
        })
        .collect::<std::io::Result<BTreeMap<_, _>>>()
        .map_err(|error| mcp::FlowError::new(mcp::ErrorKind::Setup, error.to_string()))?;
    // Connect and validate every tool/flow before any resource is launched.
    for resource in &project.plan.resources {
        let host = project
            .plan
            .hosts
            .iter()
            .find(|h| h.name == resource.host)
            .ok_or("unknown resource host")?;
        let client = tokio::time::timeout(
            Duration::from_millis(resource.timeout_ms),
            factory.connect(project, host, resource),
        )
        .await
        .map_err(|_| "project MCP connection timed out")??;
        let alias = format!("project:{}", resource.name);
        sessions.lock().await.insert(
            alias.clone(),
            Connected {
                client,
                real_id: String::new(),
                device: Value::Null,
            },
        );
        let mut guard = sessions.lock().await;
        let connection = guard.get_mut(&alias).unwrap();
        let tools = tokio::time::timeout(
            Duration::from_millis(resource.timeout_ms),
            mcp::initialize(connection.client.as_mut()),
        )
        .await
        .map_err(|_| "project MCP initialization timed out")??;
        for capability in &resource.requires {
            for name in capability_tools(*capability) {
                if !tools.contains_key(*name) {
                    return Err(format!(
                        "resource {} requires unavailable tool {name}",
                        resource.name
                    )
                    .into());
                }
            }
        }
        if let Preparation::Launch { url: Some(url), .. } = &resource.prepare {
            let url = project
                .resolve(url)
                .map_err(|_| "cannot resolve Chrome launch URL")?;
            for (name, args) in [
                ("mobile_boot_device", json!({"deviceId":"chrome:browser"})),
                (
                    "mobile_open_url",
                    json!({"deviceId":"chrome:page", "url":url}),
                ),
            ] {
                let schema = tools.get(name).ok_or_else(|| {
                    format!(
                        "resource {} requires unavailable tool {name}",
                        resource.name
                    )
                })?;
                mcp::validate_arguments(schema, &args)?;
            }
            for name in ["mobile_boot_device", "mobile_open_url"] {
                if !tools.contains_key(name) {
                    return Err(format!(
                        "resource {} requires unavailable tool {name}",
                        resource.name
                    )
                    .into());
                }
            }
        }
        if let Some(config) = &configs[&resource.name] {
            let name = mcp::WEB_TEST_PROXY;
            let schema = tools.get(name).ok_or_else(|| {
                format!(
                    "resource {} requires unavailable tool {name}",
                    resource.name
                )
            })?;
            mcp::validate_arguments(schema, &proxy_configuration(config))?;
        }
        if !tools.contains_key("mobile_list_available_devices") {
            return Err("project resource discovery tool unavailable".into());
        }
        let capability_schema = tools
            .get("mobile_get_capabilities")
            .ok_or("project capability tool unavailable")?;
        mcp::validate_arguments(capability_schema, &json!({"deviceId":"project-preflight"}))?;
        devices.insert(
            resource.name.clone(),
            json!({"id":alias,"platform":platform(resource.kind)}),
        );
        if let Some(common) = &mut common_tools {
            common.retain(|name, schema| tools.get(name) == Some(schema));
        } else {
            common_tools = Some(tools);
        }
    }
    let tools = common_tools.unwrap_or_default();
    let named_devices = named_resource_devices(project, &devices)?;
    for case in &cases {
        let mut runner = FlowRunner::new(None, directory.to_owned());
        runner.set_environment(environment.clone());
        runner.set_named_resources(named_devices.clone());
        runner
            .preflight_case(&case.flow, &case.profile, tools.clone())
            .map_err(|error| sanitize_case_error(error, &case.flow, &case.profile, &environment))?;
    }
    // Discover attached targets before launching any owned Chrome contexts.
    for launching in [false, true] {
        for resource in &project.plan.resources {
            let is_launch = matches!(resource.prepare, Preparation::Launch { .. });
            if is_launch != launching {
                continue;
            }
            let alias = format!("project:{}", resource.name);
            let limit = Duration::from_millis(resource.timeout_ms);
            let mut guard = sessions.lock().await;
            let connection = guard.get_mut(&alias).unwrap();
            if is_launch {
                if let Some(config) = &configs[&resource.name] {
                    let configured = invoke(
                        connection.client.as_mut(),
                        mcp::WEB_TEST_PROXY,
                        proxy_configuration(config),
                        limit,
                    )
                    .await?;
                    if configured
                        .pointer("/structuredContent/configured")
                        .and_then(Value::as_bool)
                        != Some(true)
                    {
                        return Err(format!(
                            "resource {} driver did not confirm its test proxy",
                            resource.name
                        )
                        .into());
                    }
                }
                invoke(
                    connection.client.as_mut(),
                    "mobile_boot_device",
                    json!({"deviceId":"chrome:browser"}),
                    limit,
                )
                .await?;
            }
            let mut device = discover_device(connection.client.as_mut(), resource).await?;
            connection.real_id = device["id"]
                .as_str()
                .ok_or("discovered device ID missing")?
                .to_owned();
            let id = connection.real_id.clone();
            let result = invoke(
                connection.client.as_mut(),
                "mobile_get_capabilities",
                json!({"deviceId":id}),
                limit,
            )
            .await?;
            let report = result
                .get("structuredContent")
                .ok_or("project capability report missing")?;
            let host = project
                .plan
                .hosts
                .iter()
                .find(|host| host.name == resource.host)
                .ok_or("unknown project host")?;
            let expected_host = match host.platform {
                HostPlatform::Macos => "macos",
                HostPlatform::Windows => "windows",
                HostPlatform::Linux => "linux",
            };
            if report["hostOs"].as_str() != Some(expected_host) {
                return Err(format!(
                    "resource {} host OS does not match declared host",
                    resource.name
                )
                .into());
            }
            if report["deviceId"].as_str() != Some(id.as_str()) {
                return Err(format!(
                    "resource {} capability device identity mismatch",
                    resource.name
                )
                .into());
            }
            for capability in &resource.requires {
                let name = match capability {
                    Capability::Discovery => "discovery",
                    Capability::Launch => "launch",
                    Capability::Input => "input",
                    Capability::Screenshot => "screenshot",
                    Capability::Elements => "elements",
                };
                let status = report["capabilities"][name]["status"]
                    .as_str()
                    .unwrap_or("unknown");
                if status != "available" {
                    return Err(format!(
                        "resource {} requires {name}, capability status {}",
                        resource.name,
                        if ["blocked", "unsupported", "unknown"].contains(&status) {
                            status
                        } else {
                            "unknown"
                        }
                    )
                    .into());
                }
            }
            device["id"] = json!(alias);
            connection.device = device.clone();
            devices.insert(resource.name.clone(), device);
        }
    }
    for resource in &project.plan.resources {
        if let Preparation::Launch { url: Some(url), .. } = &resource.prepare {
            let url = project
                .resolve(url)
                .map_err(|_| "cannot resolve Chrome launch URL")?;
            let mut guard = sessions.lock().await;
            let connection = guard
                .get_mut(&format!("project:{}", resource.name))
                .unwrap();
            let id = connection.real_id.clone();
            invoke(
                connection.client.as_mut(),
                "mobile_open_url",
                json!({"deviceId":id,"url":url}),
                Duration::from_millis(resource.timeout_ms),
            )
            .await?;
        }
    }
    let named_devices = named_resource_devices(project, &devices)?;
    if project
        .recording
        .as_ref()
        .is_some_and(|config| config.start == StartPhase::AfterPrepare)
    {
        recording::start(recorder.as_ref().ok_or("recorder setup missing")?).await?;
    }
    let mut reports = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        let default = named_devices
            .get(&case.target)
            .ok_or("project target is not prepared")?
            .clone();
        let router = Router {
            sessions: Arc::clone(&sessions),
            devices: devices.clone(),
            tools: tools.clone(),
        };
        let mut runner = FlowRunner::new(
            Some(Box::new(router)),
            directory.join(format!("case-{:03}", index + 1)),
        );
        runner.set_environment(environment.clone());
        runner.set_named_resources(named_devices.clone());
        runner.set_default_device(default);
        if let Some(recorder) = &recorder {
            if let Some(sink) = recorder
                .lock()
                .await
                .step_sink(index + 1, case.target.clone())
            {
                runner.set_step_event_sink(sink);
            }
        }
        let mut report = runner
            .run(&case.flow, &case.profile, confirmed, case_timeout)
            .await;
        if let Err(error) = runner.annotate_video(&report).await {
            report.status = "FAIL".into();
            if let Some(existing) = &mut report.error {
                existing.push_str(&format!("; {error}"));
            } else {
                report.error = Some(error.to_string());
                report.error_kind = Some("artifact".into());
            }
        }
        let case_dir = directory.join(format!("case-{:03}", index + 1));
        robot_flow::report::save_report(&report, &case_dir)?;
        let snapshot = case_dir.join("expanded-flow.yaml");
        if snapshot.is_file() {
            std::fs::copy(snapshot, case_dir.join("scenario.yaml"))
                .map_err(|error| error.to_string())?;
        }
        if let Some(root) = history_root {
            let history = robot_report::Store::open(root).and_then(|store| {
                store.ingest_flow(robot_report::FlowIngest {
                    name: report.name.clone(),
                    source: "cli".into(),
                    case_dir: case_dir.clone(),
                    app_id: Some(case.profile.app_id.clone())
                        .filter(|value| !value.trim().is_empty()),
                    url: None,
                })
            });
            if history.is_err() {
                eprintln!("History not updated for project case");
            }
        }
        let failed = report.status == "FAIL";
        reports.push(report);
        if failed {
            break;
        }
    }
    Ok(SessionResult { reports })
}
