use super::*;

#[tokio::test]
async fn positional_selectors_target_unnamed_duplicates_for_tap_input_and_scroll() {
    for (position, expected) in [(json!("first"), 10), (json!(2), 210), (json!("last"), 210)] {
        for action in ["tap_element", "focus_field", "scroll"] {
            let mut first = node("");
            first["resourceId"] = Value::Null;
            first["className"] = json!(if action == "scroll" {
                "scrollView"
            } else {
                "button"
            });
            let mut second = first.clone();
            second["bounds"]["left"] = json!(210);
            second["bounds"]["right"] = json!(310);
            let state = Arc::new(Mutex::new(State {
                source_selection: true,
                queries: VecDeque::from([json!([first, second])]),
                ..State::default()
            }));
            let report = run(
                &flow(&format!(
                    "  - {action}: {{source: flutter, class_contains: {}, position: {position}}}",
                    if action == "scroll" {
                        "scrollView"
                    } else {
                        "button"
                    }
                )),
                &Profile::default(),
                state.clone(),
            )
            .await;
            assert_eq!(
                report.status, "PASS",
                "{position} {action}: {:?}",
                report.error
            );
            let state = state.lock().unwrap();
            let query = state
                .calls
                .iter()
                .find(|c| c["params"]["name"] == "mobile_list_elements_on_screen")
                .unwrap();
            assert_eq!(query["params"]["arguments"]["source"], "flutter");
            let call = state
                .calls
                .iter()
                .find(|c| {
                    c["params"]["name"]
                        == if action == "scroll" {
                            "mobile_swipe_on_screen"
                        } else {
                            "mobile_click_on_screen_at_coordinates"
                        }
                })
                .unwrap();
            let x = if action == "scroll" { "startX" } else { "x" };
            assert_eq!(
                call["params"]["arguments"][x].as_f64(),
                Some(f64::from(expected + 50))
            );
        }
    }
}

#[tokio::test]
async fn invalid_positions_fail_before_any_action() {
    for position in [json!(0), json!(-1), json!("second"), json!(1.5)] {
        let state = Arc::new(Mutex::new(State::default()));
        let report = run(
            &flow(&format!(
                "  - tap_element: {{class_contains: button, position: {position}}}"
            )),
            &Profile::default(),
            state.clone(),
        )
        .await;
        assert_ne!(report.status, "PASS");
        assert!(report.error.unwrap().contains("position"));
        assert!(state.lock().unwrap().calls.is_empty());
    }
}
