use async_trait::async_trait;
use robot_flow::mcp::{self, Mcp};
use robot_project::{Project, init, plan::Preparation, session::SessionFactory};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::Mutex as AsyncMutex;

struct Temp(PathBuf);

impl Temp {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("project-session-cleanup-{}", uuid::Uuid::new_v4()));
        init(&path).unwrap();
        fs::write(
            path.join("project.yaml"),
            "version: 1\nkind: project\nname: attached-user-app\nhosts:\n  - {name: local, platform: macos, binding: {kind: local}}\nresources:\n  - {name: user-app, kind: macos, host: local, display: {host: local}, prepare: {kind: attach}, timeout_ms: 5000}\nflows:\n  - path: flows/smoke.yaml\n    bindings: [{target: user-app}]\n",
        )
        .unwrap();
        fs::write(
            path.join("flows/smoke.yaml"),
            "version: 2\nname: attached-user-app\nsteps:\n  - tap: {x: 12, y: 34}\n",
        )
        .unwrap();
        Self(path)
    }

    fn project(&self) -> Project {
        Project::load(&self.0).unwrap()
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct FakeFactory {
    project_calls: Arc<Mutex<Vec<(String, Value)>>>,
    project_closes: Arc<AtomicUsize>,
    unrelated_calls: Arc<Mutex<Vec<String>>>,
    unrelated_closes: Arc<AtomicUsize>,
    // Keep a separate fake client alive alongside the project run. It is not
    // returned from connect and represents a session owned by another project.
    _unrelated_session: AsyncMutex<FakeClient>,
}

impl FakeFactory {
    fn new() -> Self {
        let unrelated_calls = Arc::new(Mutex::new(Vec::new()));
        let unrelated_closes = Arc::new(AtomicUsize::new(0));
        Self {
            project_calls: Arc::new(Mutex::new(Vec::new())),
            project_closes: Arc::new(AtomicUsize::new(0)),
            unrelated_calls: Arc::clone(&unrelated_calls),
            unrelated_closes: Arc::clone(&unrelated_closes),
            _unrelated_session: AsyncMutex::new(FakeClient {
                project_calls: None,
                unrelated_calls: Some(unrelated_calls),
                close_count: unrelated_closes,
            }),
        }
    }
}

#[async_trait]
impl SessionFactory for FakeFactory {
    async fn connect(
        &self,
        _: &Project,
        _: &robot_project::plan::Host,
        resource: &robot_project::plan::Resource,
    ) -> mcp::Result<Box<dyn Mcp>> {
        assert_eq!(resource.name, "user-app");
        assert!(matches!(resource.prepare, Preparation::Attach {}));
        Ok(Box::new(FakeClient {
            project_calls: Some(Arc::clone(&self.project_calls)),
            unrelated_calls: None,
            close_count: Arc::clone(&self.project_closes),
        }))
    }
}

type ProjectToolCalls = Arc<Mutex<Vec<(String, Value)>>>;

struct FakeClient {
    project_calls: Option<ProjectToolCalls>,
    unrelated_calls: Option<Arc<Mutex<Vec<String>>>>,
    close_count: Arc<AtomicUsize>,
}

#[async_trait]
impl Mcp for FakeClient {
    async fn request(&mut self, method: &str, params: Value) -> mcp::Result<Value> {
        match method {
            "initialize" => Ok(json!({"protocolVersion":"2025-11-25"})),
            "tools/list" => Ok(json!({"tools":[
                {"name":"mobile_list_available_devices","inputSchema":{"type":"object"}},
                {"name":"mobile_get_capabilities","inputSchema":{"type":"object"}},
                {"name":"mobile_click_on_screen_at_coordinates","inputSchema":{"type":"object"}}
            ]})),
            "tools/call" => {
                let name = params["name"].as_str().ok_or("tool name missing")?;
                if let Some(calls) = &self.project_calls {
                    calls
                        .lock()
                        .unwrap()
                        .push((name.to_owned(), params["arguments"].clone()));
                }
                if let Some(calls) = &self.unrelated_calls {
                    calls.lock().unwrap().push(name.to_owned());
                }
                match name {
                    "mobile_list_available_devices" => Ok(json!({"structuredContent":{"devices":[{
                        "id":"macos:user-owned-app",
                        "name":"user-owned-app",
                        "platform":"macos",
                        "state":"available"
                    }]}})),
                    "mobile_get_capabilities" => Ok(json!({"structuredContent":{
                        "hostOs":"macos",
                        "deviceId":params["arguments"]["deviceId"],
                        "capabilities":{}
                    }})),
                    "mobile_click_on_screen_at_coordinates" => Ok(json!({"content":[]})),
                    _ => Err(format!("unexpected tool {name}").into()),
                }
            }
            _ => Err(format!("unexpected MCP method {method}").into()),
        }
    }

    async fn notify(&mut self, _: &str, _: Value) -> mcp::Result<()> {
        Ok(())
    }

    async fn close(&mut self) -> mcp::Result<()> {
        self.close_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn attached_user_resource_closes_only_its_project_mcp_session() {
    let temp = Temp::new();
    let project = temp.project();
    let factory = FakeFactory::new();
    let output = temp.0.join("artifacts");

    let result =
        robot_project::session::run(&project, &factory, &output, &[], Duration::from_secs(5))
            .await
            .unwrap();

    assert_eq!(result.reports.len(), 1);
    assert_eq!(result.reports[0].status, "PASS");
    assert_eq!(factory.project_closes.load(Ordering::SeqCst), 1);

    let project_calls = factory.project_calls.lock().unwrap();
    assert!(project_calls.iter().any(|(name, args)| {
        name == "mobile_click_on_screen_at_coordinates"
            && args["deviceId"] == "macos:user-owned-app"
    }));
    for (name, _) in project_calls.iter() {
        assert!(
            !matches!(
                name.as_str(),
                "mobile_terminate_app"
                    | "mobile_terminate_all_apps"
                    | "mobile_kill_app"
                    | "mobile_uninstall_app"
                    | "mobile_boot_device"
                    | "mobile_stop_device"
                    | "mobile_shutdown_device"
                    | "mobile_kill_device"
            ),
            "attached user resource received a destructive action: {name}"
        );
    }

    assert!(factory.unrelated_calls.lock().unwrap().is_empty());
    assert_eq!(factory.unrelated_closes.load(Ordering::SeqCst), 0);
}
