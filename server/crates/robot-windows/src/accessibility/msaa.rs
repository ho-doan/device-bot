//! Explicit MSAA mode keeps UIA/MSAA comparisons reproducible without merging
//! duplicate representations of the same control or hiding UIA failures.
use crate::{MAX_UI_DEPTH, MAX_UI_NODES, UIA_QUERY_TIMEOUT, WindowsRobot, parse_uia_elements};
use robot_core::{RobotError, UiElement};

pub(crate) fn use_msaa(value: Option<&str>) -> Result<bool, RobotError> {
    match value {
        None | Some("uia") => Ok(false),
        Some("msaa") => Ok(true),
        Some(_) => Err(RobotError::InvalidInput(
            "DEVICE_BOT_WINDOWS_ACCESSIBILITY must be uia or msaa".into(),
        )),
    }
}

fn script(pid: Option<u32>, hwnd: Option<i64>, max: Option<usize>) -> String {
    format!(
        r#"$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName Accessibility
Add-Type -ReferencedAssemblies @('System.dll', 'System.Core.dll', [Accessibility.IAccessible].Assembly.Location) -TypeDefinition @'
{helper}
'@
$rows = @([DeviceBotMsaa]::Read([uint32]{pid}, [int64]{hwnd}, {max}, {nodes}, {depth}))
ConvertTo-Json -InputObject $rows -Depth 4 -Compress
"#,
        helper = include_str!("msaa_helper.cs"),
        pid = pid.unwrap_or(0),
        hwnd = hwnd.unwrap_or(0),
        max = max.unwrap_or(MAX_UI_NODES).min(MAX_UI_NODES),
        nodes = MAX_UI_NODES,
        depth = MAX_UI_DEPTH,
    )
}

impl WindowsRobot {
    pub(crate) async fn msaa_elements(
        &self,
        pid: Option<u32>,
        hwnd: Option<i64>,
        max: Option<usize>,
    ) -> Result<Vec<UiElement>, RobotError> {
        if max == Some(0) {
            return Ok(vec![]);
        }
        let output =
            tokio::time::timeout(UIA_QUERY_TIMEOUT, self.powershell(script(pid, hwnd, max)))
                .await
                .map_err(|_| {
                    RobotError::CommandFailed(
                        "Windows MSAA helper exceeded its 10 second startup/traversal deadline"
                            .into(),
                    )
                })??;
        parse_uia_elements(&output).map_err(|e| {
            RobotError::CommandFailed(format!("Windows MSAA returned invalid elements: {e}"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_selection_is_explicit() {
        assert!(!use_msaa(None).unwrap());
        assert!(!use_msaa(Some("uia")).unwrap());
        assert!(use_msaa(Some("msaa")).unwrap());
        assert!(use_msaa(Some("invalid")).is_err());
    }

    #[tokio::test]
    async fn empty_query_never_launches_helper() {
        assert!(
            WindowsRobot::new("artifacts")
                .msaa_elements(Some(42), Some(123), Some(0))
                .await
                .unwrap()
                .is_empty()
        );
    }

    // Exercises the actual C# traversal with managed IAccessible providers,
    // including both object children and integer child IDs. No Flutter needed.
    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn managed_msaa_provider_contract() {
        let helper = include_str!("msaa_helper.cs");
        let fixture = include_str!("../../tests/msaa_provider.cs");
        let script = format!(
            r#"$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName Accessibility
Add-Type -ReferencedAssemblies @('System.dll', 'System.Core.dll', [Accessibility.IAccessible].Assembly.Location) -TypeDefinition @'
{helper}
{fixture}
'@
[MsaaProviderTests]::Run()
"#
        );
        WindowsRobot::new("artifacts")
            .powershell(script)
            .await
            .unwrap();
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    #[ignore = "requires a running release app, HWND and expected visible control labels"]
    async fn msaa_release_controls_without_bridge() {
        let pid = std::env::var("DEVICE_BOT_MSAA_PID")
            .unwrap()
            .parse()
            .unwrap();
        let hwnd = std::env::var("DEVICE_BOT_MSAA_HWND")
            .unwrap()
            .parse()
            .unwrap();
        let expected: Vec<String> = serde_json::from_str(
            &std::env::var("DEVICE_BOT_MSAA_LABELS").expect("JSON array of visible control labels"),
        )
        .unwrap();
        assert!(
            !expected.is_empty(),
            "at least one control label is required"
        );
        let elements = WindowsRobot::new("artifacts")
            .msaa_elements(Some(pid), Some(hwnd), None)
            .await
            .unwrap();
        for label in expected {
            assert!(
                elements.iter().any(|e| e.text.as_deref() == Some(&label)),
                "missing visible control {label:?} in MSAA snapshot: {elements:?}"
            );
        }
    }
}
