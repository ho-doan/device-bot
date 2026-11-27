use super::*;
use robot_flow::mcp::StdioMcp;
#[tokio::test]
async fn cancelled_query_allows_recording_cleanup_with_late_or_partial_reply() {
    let script = r#"import sys,json,time
partial=sys.argv[1]=='1'
for line in sys.stdin:
 m=json.loads(line)
 if 'id' not in m: continue
 reply=json.dumps({'jsonrpc':'2.0','id':m['id'],'result':{'method':m['method']}})
 if m['method']=='slow_query':
  if partial:
   cut=len(reply)//2
   sys.stdout.write(reply[:cut]);sys.stdout.flush()
   time.sleep(0.15)
   print(reply[cut:],flush=True)
  else:
   time.sleep(0.15)
   print(reply,flush=True)
 else: print(reply,flush=True)
"#;
    for partial in [false, true] {
        let mut client = StdioMcp::spawn(
            "python3",
            &[
                "-u".into(),
                "-c".into(),
                script.into(),
                if partial { "1" } else { "0" }.into(),
            ],
            Duration::from_secs(2),
        )
        .unwrap();
        client.request("ready", json!({})).await.unwrap();
        let cancelled = tokio::time::timeout(
            Duration::from_millis(30),
            client.request("slow_query", json!({})),
        )
        .await;
        assert!(cancelled.is_err());
        let cleanup = client
            .request("mobile_stop_screen_recording", json!({}))
            .await;
        assert_eq!(
            cleanup.as_ref().ok(),
            Some(&json!({"method":"mobile_stop_screen_recording"})),
            "partial={partial}: {cleanup:?}"
        );
        client.close().await.unwrap();
    }
}
#[tokio::test]
async fn unrelated_response_ids_still_fail_protocol_validation() {
    for reply in [0, 999] {
        let script = format!(
            "import sys,json\nfor line in sys.stdin:\n m=json.loads(line)\n print(json.dumps({{'jsonrpc':'2.0','id':{reply},'result':{{}}}}),flush=True)\n"
        );
        let mut client = StdioMcp::spawn(
            "python3",
            &["-u".into(), "-c".into(), script],
            Duration::from_secs(2),
        )
        .unwrap();
        let error = client
            .request("query", json!({}))
            .await
            .unwrap_err()
            .to_string();
        assert!(
            error.to_string().contains("MCP response ID mismatch"),
            "{error}"
        );
        client.close().await.unwrap();
    }
}
#[tokio::test]
async fn real_pipe_lifecycle_and_shutdown() {
    let script = "import sys,json\nfor line in sys.stdin:\n m=json.loads(line)\n if 'id' not in m: continue\n r={'protocolVersion':'2025-11-25'} if m['method']=='initialize' else {'tools':[]}\n print(json.dumps({'jsonrpc':'2.0','id':m['id'],'result':r}),flush=True)\n";
    let mut client = StdioMcp::spawn(
        "python3",
        &["-u".into(), "-c".into(), script.into()],
        Duration::from_secs(2),
    )
    .unwrap();
    assert!(mcp::initialize(&mut client).await.unwrap().is_empty());
    client.close().await.unwrap();
}
#[tokio::test]
async fn pipe_eof_timeout_and_protocol_errors_are_distinct() {
    for (script, expected) in [
        ("import sys;sys.exit(0)", "EOF"),
        ("import time;time.sleep(3)", "timeout"),
        ("print('bad-json',flush=True)", "invalid MCP JSON"),
    ] {
        let mut client = StdioMcp::spawn(
            "python3",
            &["-u".into(), "-c".into(), script.into()],
            if expected == "timeout" {
                Duration::from_millis(100)
            } else {
                Duration::from_secs(5)
            },
        )
        .unwrap();
        let error = client
            .request("initialize", json!({}))
            .await
            .unwrap_err()
            .to_string();
        assert!(error.to_string().contains(expected), "{error}");
        client.close().await.unwrap();
    }
}
