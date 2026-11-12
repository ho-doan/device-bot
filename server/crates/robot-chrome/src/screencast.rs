//! Event-driven CDP fallback. Never calls Page.captureScreenshot.
use super::*;
use robot_core::{VideoFrame, VideoStream};
use tokio::sync::mpsc;

fn failure(error: impl std::fmt::Display) -> RobotError {
    RobotError::CommandFailed(format!("Chrome Live fallback: {error}"))
}

impl ChromeRobot {
    pub(super) async fn start_screencast(&self, id: &str) -> Result<VideoStream, RobotError> {
        let target = self.page(id).await?; // Enforce case ownership before opening a capture connection.
        self.cdp(&target, "Page.bringToFront", json!({})).await?;
        let endpoint = self.running_browser_endpoint().await?;
        let mut connection = connection::BrowserConnection::connect(&endpoint).await?;
        let attached = connection
            .request(
                "Target.attachToTarget",
                json!({"targetId":target.id,"flatten":true}),
                None,
            )
            .await?;
        let session = attached["sessionId"]
            .as_str()
            .ok_or_else(|| failure("missing capture session"))?
            .to_owned();
        let metrics = connection.request(
            "Runtime.evaluate",
            json!({"expression":"({width:innerWidth,height:innerHeight})", "returnByValue":true}),
            Some(session.clone()),
        ).await?;
        let width = metrics["result"]["value"]["width"]
            .as_u64()
            .filter(|v| *v > 0)
            .ok_or_else(|| failure("missing viewport width"))? as u32;
        let height = metrics["result"]["value"]["height"]
            .as_u64()
            .filter(|v| *v > 0)
            .ok_or_else(|| failure("missing viewport height"))? as u32;
        let mut encoder = Command::new(&self.ffmpeg_path)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-probesize",
                "32",
                "-analyzeduration",
                "0",
                "-f",
                "image2pipe",
                "-framerate",
                "30",
                "-vcodec",
                "mjpeg",
                "-i",
                "pipe:0",
                "-an",
                "-vf",
                "scale=trunc(iw/2)*2:trunc(ih/2)*2",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-tune",
                "zerolatency",
                "-profile:v",
                "baseline",
                "-pix_fmt",
                "yuv420p",
                "-g",
                "30",
                "-bf",
                "0",
                "-x264-params",
                "aud=1:repeat-headers=1",
                "-f",
                "h264",
                "pipe:1",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                failure(format!(
                    "FFmpeg with libx264 is required (DEVICE_BOT_FFMPEG_PATH): {e}"
                ))
            })?;
        let mut stdin = encoder.stdin.take().unwrap();
        let mut stdout = encoder.stdout.take().unwrap();
        // libx264/yuv420p requires even dimensions; advertise the encoded size.
        let width = width.max(2) & !1;
        let height = height.max(2) & !1;
        let start_id = REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        // Read the start response and frame events together: request() discards
        // unrelated events and could otherwise lose a static page's first frame.
        let start = json!({
            "id":start_id, "sessionId":session, "method":"Page.startScreencast",
            "params":{"format":"jpeg", "quality":75, "maxWidth":width,
                "maxHeight":height, "everyNthFrame":1}
        });
        connection
            .socket
            .send(Message::Text(start.to_string().into()))
            .await
            .map_err(failure)?;
        let (sender, frames) = mpsc::channel(2);
        let output = sender.clone();
        let mut reader = tokio::spawn(async move {
            let mut buffer = Vec::new();
            let mut chunk = [0u8; 16384];
            let mut pts = 0;
            loop {
                let size = stdout.read(&mut chunk).await.map_err(failure)?;
                if size == 0 {
                    return Err(failure("encoder stopped"));
                }
                buffer.extend_from_slice(&chunk[..size]);
                if buffer.len() > 16 * 1024 * 1024 {
                    return Err(failure("encoder frame exceeds limit"));
                }
                while let Some(unit) = take_access_unit(&mut buffer) {
                    let keyframe = nal_types(&unit).any(|kind| kind == 5);
                    let frame = VideoFrame {
                        data: unit,
                        pts_us: pts,
                        keyframe,
                    };
                    pts += 33_333;
                    if output.send(Ok(frame)).await.is_err() {
                        return Ok::<(), RobotError>(());
                    }
                }
            }
        });
        tokio::spawn(async move {
            let mut last = None::<Vec<u8>>;
            let mut ticks = interval(Duration::from_micros(33_333));
            ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);
            let result = async {
                loop {
                    tokio::select! {
                        _ = sender.closed() => return Ok(()),
                        result = &mut reader => return result.map_err(failure)?,
                        frame = connection.socket.next() => {
                            let frame = frame.ok_or_else(|| failure("capture connection closed"))?
                                .map_err(failure)?;
                            if frame.is_close() {
                                return Err(failure("capture connection closed"));
                            }
                            if !frame.is_text() { continue; }
                            let event: Value = serde_json::from_str(frame.to_text().map_err(failure)?)
                                .map_err(failure)?;
                            if event["id"] == start_id && event.get("error").is_some() {
                                return Err(failure(event["error"].to_string()));
                            }
                            if event["sessionId"] != session || event["method"] != "Page.screencastFrame" {
                                continue;
                            }
                            let encoded = event["params"]["data"].as_str()
                                .ok_or_else(|| failure("missing frame data"))?;
                            if encoded.len() > 20 * 1024 * 1024 {
                                return Err(failure("capture frame exceeds limit"));
                            }
                            last = Some(base64::engine::general_purpose::STANDARD.decode(encoded).map_err(failure)?);
                            let ack = json!({
                                "id":REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed),
                                "sessionId":session, "method":"Page.screencastFrameAck",
                                "params":{"sessionId":event["params"]["sessionId"]}
                            });
                            connection.socket.send(Message::Text(ack.to_string().into())).await.map_err(failure)?;
                        },
                        _ = ticks.tick() => {
                            // Repeat the last event frame to maintain the encoder clock on
                            // static pages. No new screenshot is requested from Chrome.
                            if let Some(jpeg) = &last {
                                tokio::time::timeout(Duration::from_secs(3), stdin.write_all(jpeg))
                                    .await.map_err(failure)?.map_err(failure)?;
                            }
                        }
                    }
                }
            }.await;
            if let Err(error) = result {
                let _ = sender.try_send(Err(error));
            }
            let _ = connection
                .request("Page.stopScreencast", json!({}), Some(session))
                .await;
            drop(stdin);
            let _ = encoder.kill().await;
            reader.abort();
        });
        Ok(VideoStream {
            width,
            height,
            frames,
        })
    }
}
fn nal_types(data: &[u8]) -> impl Iterator<Item = u8> + '_ {
    data.windows(4)
        .filter(|bytes| bytes[..3] == [0, 0, 1])
        .map(|bytes| bytes[3] & 31)
}
fn take_access_unit(buffer: &mut Vec<u8>) -> Option<Vec<u8>> {
    let mut boundaries = buffer
        .windows(5)
        .enumerate()
        .filter(|(_, b)| b[..4] == [0, 0, 0, 1] && b[4] & 31 == 9)
        .map(|(i, _)| i);
    boundaries.next()?;
    let end = boundaries.next()?;
    Some(buffer.drain(..end).collect())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frames_split_on_aud_and_preserve_sps_and_idr() {
        let mut data = vec![
            0, 0, 0, 1, 9, 240, 0, 0, 0, 1, 0x67, 1, 0, 0, 1, 0x65, 2, 0, 0, 0, 1, 9, 240,
        ];
        let frame = take_access_unit(&mut data).unwrap();
        assert!(nal_types(&frame).any(|n| n == 5));
        assert_eq!(data, vec![0, 0, 0, 1, 9, 240]);
        assert!(take_access_unit(&mut data).is_none());
    }
}

#[cfg(test)]
mod physical_tests {
    use super::*;
    #[tokio::test]
    #[ignore = "requires Chrome at 9222 and FFmpeg libx264"]
    async fn insecure_http_screencast_encodes_h264_and_scrolls_fifty_pixels() {
        let route = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
        route.connect("192.0.2.1:9").unwrap();
        let ip = route.local_addr().unwrap().ip();
        let listener = tokio::net::TcpListener::bind((ip, 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut request = [0; 4096];
                    let _ = socket.read(&mut request).await;
                    let body = "<!doctype html><html><body style='margin:0;height:5000px;background:linear-gradient(red,blue)'><input id='probe' value='hello'><button id='target'>Tap</button></body></html>";
                    let _=socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await;
                });
            }
        });
        let mut robot = ChromeRobot::new("127.0.0.1:9222");
        robot.ffmpeg_path =
            std::env::var("DEVICE_BOT_FFMPEG_PATH").unwrap_or("/opt/homebrew/bin/ffmpeg".into());
        robot.artifact_dir = std::env::temp_dir().join("device-bot-live-fallback-test");
        robot.boot_device("chrome:browser").await.unwrap();
        let id = robot.list_devices().await.unwrap()[0].id.clone();
        let result = async {
            robot
                .open_url(&id, &format!("http://{address}"), None)
                .await?;
            tokio::time::sleep(Duration::from_millis(400)).await;
            if robot.negotiate_native_video(&id, "unused").await?.is_some() {
                return Err(failure("HTTP unexpectedly used primary capture"));
            }
            let mut stream = robot.start_video_stream(&id).await?;
            let frame = tokio::time::timeout(Duration::from_secs(15), stream.frames.recv())
                .await
                .map_err(failure)?
                .ok_or_else(|| failure("no frame"))??;
            assert!(frame.keyframe);
            assert!(nal_types(&frame.data).any(|n| n == 5));
            robot
                .scroll(&id, Point { x: 200., y: 200. }, 0., 50.)
                .await?;
            tokio::time::sleep(Duration::from_millis(400)).await;
            let target = robot.page(&id).await?;
            let response = robot
                .cdp(
                    &target,
                    "Runtime.evaluate",
                    json!({"expression":"scrollY","returnByValue":true}),
                )
                .await?;
            assert_eq!(response["result"]["value"], 50);
            println!(
                "HTTP LAN {address}: H.264 {}x{}, {} bytes; scrollY=50",
                stream.width,
                stream.height,
                frame.data.len()
            );
            drop(stream);
            tokio::time::sleep(Duration::from_millis(400)).await;
            Ok::<(), RobotError>(())
        }
        .await;
        let _ = robot.close_session().await;
        server.abort();
        result.unwrap();
    }
}
