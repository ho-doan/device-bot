//! HTTP/1 test proxy for one owned Chrome context. It never resolves hostnames
//! or connects to a destination that was not explicitly declared for the case.

use http_body_util::{BodyExt, Full, combinators::UnsyncBoxBody};
use hyper::{
    Method, Request, Response, StatusCode, Uri,
    body::{Bytes, Incoming},
    client::conn::http1 as client,
    header::{self, HeaderMap, HeaderName, HeaderValue},
    server::conn::http1 as server,
    service::service_fn,
};
use hyper_util::rt::TokioIo;
use robot_core::{RobotError, WebFault, WebFaultState, WebFaultTransition, WebProxyConfig};
use std::{
    collections::HashMap,
    future::pending,
    io,
    net::{Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::copy_bidirectional,
    net::{TcpListener, TcpStream},
    sync::{Semaphore, oneshot, watch},
    task::{JoinHandle, JoinSet},
    time::{sleep, timeout},
};

type Body = UnsyncBoxBody<Bytes, hyper::Error>;
type Reply = io::Result<Response<Body>>;
const CONNECT_LIMIT: Duration = Duration::from_secs(5);
const RESPONSE_LIMIT: Duration = Duration::from_secs(60);
const MAX_HTTP_CONNECTIONS: usize = 64;
const MAX_TUNNELS: usize = 32;

#[derive(Default)]
struct Policy {
    active: WebFaultState,
    next_tunnel: u64,
    upstream_tunnels: HashMap<u64, oneshot::Sender<()>>,
}

struct ProxyState {
    config: WebProxyConfig,
    policy: Mutex<Policy>,
    tunnel_capacity: Arc<Semaphore>,
}

pub(super) struct TestProxy {
    addr: SocketAddr,
    state: Arc<ProxyState>,
    stop: watch::Sender<bool>,
    server: JoinHandle<()>,
}

impl TestProxy {
    pub(super) async fn start(config: WebProxyConfig) -> Result<Self, RobotError> {
        config.validate()?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(proxy_error)?;
        let addr = listener.local_addr().map_err(proxy_error)?;
        let state = Arc::new(ProxyState {
            config,
            policy: Mutex::default(),
            tunnel_capacity: Arc::new(Semaphore::new(MAX_TUNNELS)),
        });
        let (stop, stopped) = watch::channel(false);
        let server = tokio::spawn(serve(listener, Arc::clone(&state), stopped));
        Ok(Self {
            addr,
            state,
            stop,
            server,
        })
    }

    pub(super) fn address(&self) -> SocketAddr {
        self.addr
    }

    pub(super) fn state(&self) -> WebFaultState {
        self.state.policy.lock().unwrap().active.clone()
    }

    pub(super) fn set(&self, fault: Option<WebFault>) -> Result<WebFaultTransition, RobotError> {
        if let Some(fault) = &fault {
            fault.validate()?;
        }
        let mut policy = self.state.policy.lock().unwrap();
        let previous = policy.active.clone();
        let mut active = WebFaultState {
            revision: previous.revision + 1,
            fault,
            hits: 0,
        };
        if active.fault == Some(WebFault::WebSocketDrop) {
            active.hits = policy
                .upstream_tunnels
                .drain()
                .map(|(_, stop)| u64::from(stop.send(()).is_ok()))
                .sum();
        }
        policy.active = active.clone();
        Ok(WebFaultTransition { previous, active })
    }
}

impl Drop for TestProxy {
    fn drop(&mut self) {
        self.stop.send_replace(true);
        self.server.abort();
    }
}

fn proxy_error(error: io::Error) -> RobotError {
    RobotError::CommandFailed(format!("could not start loopback test proxy: {error}"))
}

async fn serve(listener: TcpListener, state: Arc<ProxyState>, stopped: watch::Receiver<bool>) {
    let mut http = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else { break };
                if http.len() >= MAX_HTTP_CONNECTIONS {
                    continue;
                }
                let state = Arc::clone(&state);
                let stopped = stopped.clone();
                http.spawn(async move {
                    let handler = service_fn(move |req| handle(Arc::clone(&state), stopped.clone(), req));
                    let _ = server::Builder::new()
                        .serve_connection(TokioIo::new(stream), handler)
                        .with_upgrades().await;
                });
            }
            _ = http.join_next(), if !http.is_empty() => {},
        }
    }
}

async fn shutdown(mut stopped: watch::Receiver<bool>) {
    // wait_for also observes a stop sent before the task was polled.
    let _ = stopped.wait_for(|value| *value).await;
}

async fn connect(target: SocketAddr) -> io::Result<TcpStream> {
    timeout(CONNECT_LIMIT, TcpStream::connect(target)).await?
}

fn fixed(status: StatusCode, message: &'static str) -> Response<Body> {
    let body = Full::new(Bytes::from_static(message.as_bytes()))
        .map_err(|e| match e {})
        .boxed_unsync();
    let mut response = Response::new(body);
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn disconnected() -> io::Error {
    io::Error::new(
        io::ErrorKind::ConnectionReset,
        "injected test connection drop",
    )
}

async fn handle(
    state: Arc<ProxyState>,
    stopped: watch::Receiver<bool>,
    req: Request<Incoming>,
) -> Reply {
    let is_tunnel = req.method() == Method::CONNECT;
    if !is_tunnel && req.uri().scheme_str() != Some("http") {
        return Ok(fixed(
            StatusCode::FORBIDDEN,
            "test proxy requires an explicit HTTP destination",
        ));
    }
    // Never use Host or DNS to select where to connect.
    let Some(target) = req
        .uri()
        .authority()
        .and_then(|addr| addr.as_str().parse::<SocketAddr>().ok())
    else {
        return Ok(fixed(
            StatusCode::FORBIDDEN,
            "test proxy requires an explicit loopback address and port",
        ));
    };
    let is_upstream = target == state.config.upstream;
    if !is_upstream && !state.config.passthrough.contains(&target) {
        return Ok(fixed(
            StatusCode::FORBIDDEN,
            "destination not declared for this test proxy",
        ));
    }
    if is_tunnel {
        return tunnel(state, stopped, req, target, is_upstream).await;
    }
    // Forward CORS preflight normally, so a mock HTTP error reaches the app
    // rather than being replaced by a generic preflight failure.
    let fault = if is_upstream && req.method() != Method::OPTIONS {
        let mut policy = state.policy.lock().unwrap();
        let fault = policy
            .active
            .fault
            .clone()
            .filter(|fault| fault.http_path() == Some(req.uri().path()));
        if fault.is_some() {
            policy.active.hits += 1;
        }
        fault
    } else {
        None
    };
    match fault {
        Some(WebFault::HttpStatus { status, .. }) => {
            let mut response = fixed(StatusCode::from_u16(status).unwrap(), "injected HTTP fault");
            if let Some(origin) = req.headers().get(header::ORIGIN) {
                response
                    .headers_mut()
                    .insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin.clone());
                response.headers_mut().insert(
                    header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
                    HeaderValue::from_static("true"),
                );
                response
                    .headers_mut()
                    .insert(header::VARY, HeaderValue::from_static("Origin"));
            }
            Ok(response)
        }
        Some(WebFault::Drop { .. }) => Err(disconnected()),
        Some(WebFault::Delay { ms, .. }) => {
            sleep(Duration::from_millis(ms)).await;
            forward(req, target, stopped).await
        }
        _ => forward(req, target, stopped).await,
    }
}

struct ActiveTunnel {
    state: Arc<ProxyState>,
    id: u64,
}
impl Drop for ActiveTunnel {
    fn drop(&mut self) {
        self.state
            .policy
            .lock()
            .unwrap()
            .upstream_tunnels
            .remove(&self.id);
    }
}

async fn tunnel(
    state: Arc<ProxyState>,
    stopped: watch::Receiver<bool>,
    mut req: Request<Incoming>,
    target: SocketAddr,
    is_upstream: bool,
) -> Reply {
    let Ok(permit) = Arc::clone(&state.tunnel_capacity).try_acquire_owned() else {
        return Ok(fixed(
            StatusCode::SERVICE_UNAVAILABLE,
            "test proxy tunnel limit",
        ));
    };
    let registration = if is_upstream {
        let mut policy = state.policy.lock().unwrap();
        match &policy.active.fault {
            Some(WebFault::WebSocketReject) => {
                policy.active.hits += 1;
                return Ok(fixed(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "injected WebSocket rejection",
                ));
            }
            Some(WebFault::WebSocketDrop) => {
                policy.active.hits += 1;
                return Err(disconnected());
            }
            _ => {}
        }
        let (cut, cut_rx) = oneshot::channel();
        let id = policy.next_tunnel;
        policy.next_tunnel += 1;
        policy.upstream_tunnels.insert(id, cut);
        Some((
            ActiveTunnel {
                state: Arc::clone(&state),
                id,
            },
            cut_rx,
        ))
    } else {
        None
    };
    let (registration, cut_rx) = match registration {
        Some((active, cut)) => (Some(active), Some(cut)),
        None => (None, None),
    };
    let Ok(mut peer) = connect(target).await else {
        return Ok(fixed(
            StatusCode::BAD_GATEWAY,
            "could not reach test upstream",
        ));
    };
    let upgrade = hyper::upgrade::on(&mut req);
    tokio::spawn(async move {
        let _permit = permit;
        let _registration = registration;
        let cut = async {
            match cut_rx {
                Some(rx) => {
                    let _ = rx.await;
                }
                None => pending::<()>().await,
            }
        };
        tokio::select! {
            biased;
            _ = shutdown(stopped) => {},
            _ = cut => {},
            _ = async {
                if let Ok(upgraded) = upgrade.await {
                    let _ = copy_bidirectional(&mut TokioIo::new(upgraded), &mut peer).await;
                }
            } => {},
        }
    });
    Ok(Response::new(
        Full::<Bytes>::default()
            .map_err(|e| match e {})
            .boxed_unsync(),
    ))
}

async fn forward(
    mut req: Request<Incoming>,
    target: SocketAddr,
    stopped: watch::Receiver<bool>,
) -> Reply {
    let response = timeout(RESPONSE_LIMIT, async {
        let stream = connect(target).await?;
        let (mut sender, connection) = client::handshake(TokioIo::new(stream)).await?;
        tokio::spawn(async move {
            tokio::select! { _ = shutdown(stopped) => {}, _ = connection => {} }
        });
        let path = req
            .uri()
            .path_and_query()
            .map(|v| v.as_str())
            .unwrap_or("/");
        *req.uri_mut() = path.parse::<Uri>()?;
        strip_hop_headers(req.headers_mut());
        req.headers_mut()
            .insert(header::HOST, HeaderValue::from_str(&target.to_string())?);
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(sender.send_request(req).await?)
    })
    .await;
    match response {
        Ok(Ok(answer)) => {
            let (mut parts, body) = answer.into_parts();
            strip_hop_headers(&mut parts.headers);
            Ok(Response::from_parts(parts, body.boxed_unsync()))
        }
        Ok(Err(_)) => Ok(fixed(
            StatusCode::BAD_GATEWAY,
            "could not reach test upstream",
        )),
        Err(_) => Ok(fixed(
            StatusCode::GATEWAY_TIMEOUT,
            "test upstream did not respond",
        )),
    }
}

fn strip_hop_headers(headers: &mut HeaderMap) {
    let named: Vec<_> = headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter_map(|name| HeaderName::from_bytes(name.trim().as_bytes()).ok())
        .collect();
    for name in named {
        headers.remove(name);
    }
    for name in [
        "connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "proxy-connection",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
    ] {
        headers.remove(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

    struct Fixture {
        addr: SocketAddr,
        hits: Arc<AtomicUsize>,
        task: JoinHandle<()>,
    }
    impl Fixture {
        async fn start(echo: bool) -> Self {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
            let addr = listener.local_addr().unwrap();
            let hits = Arc::new(AtomicUsize::new(0));
            let count = Arc::clone(&hits);
            let task = tokio::spawn(async move {
                let mut children = JoinSet::new();
                while let Ok((socket, _)) = listener.accept().await {
                    let count = Arc::clone(&count);
                    children.spawn(async move {
                        let mut stream = BufReader::new(socket);
                        if echo {
                            let mut data = [0; 4];
                            if stream.read_exact(&mut data).await.is_ok() {
                                count.fetch_add(1, Ordering::Relaxed);
                                let _ = stream.write_all(&data).await;
                            }
                            let _ = stream.read_to_end(&mut Vec::new()).await;
                        } else if let Ok(line) = header_block(&mut stream).await {
                            count.fetch_add(1, Ordering::Relaxed);
                            let target = line.split_ascii_whitespace().nth(1).unwrap();
                            let response = format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{target}",
                                target.len()
                            );
                            let _ = stream.write_all(response.as_bytes()).await;
                        }
                    });
                }
            });
            Self { addr, hits, task }
        }

        fn requests(&self) -> usize {
            self.hits.load(Ordering::Relaxed)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn header_block(stream: &mut BufReader<TcpStream>) -> io::Result<String> {
        let mut line = String::new();
        let mut head = String::new();
        loop {
            line.clear();
            if stream.read_line(&mut line).await? == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            head.push_str(&line);
            if line == "\r\n" {
                return Ok(head);
            }
        }
    }

    async fn raw_request(addr: SocketAddr, text: String) -> String {
        timeout(Duration::from_secs(3), async {
            let mut socket = TcpStream::connect(addr).await.unwrap();
            socket.write_all(text.as_bytes()).await.unwrap();
            let mut buffer = Vec::new();
            if let Err(error) = socket.read_to_end(&mut buffer).await {
                assert_eq!(error.kind(), io::ErrorKind::ConnectionReset);
            }
            String::from_utf8(buffer).unwrap()
        })
        .await
        .unwrap()
    }

    async fn get(proxy: &TestProxy, target: SocketAddr, path: &str) -> String {
        raw_request(proxy.address(), format!(
            "GET http://{target}{path} HTTP/1.1\r\nHost: {target}\r\nOrigin: http://fixture.test\r\nConnection: close\r\n\r\n"
        )).await
    }

    #[tokio::test]
    async fn faults_match_only_the_declared_upstream_path_and_never_cross_sessions() {
        let api = Fixture::start(false).await;
        let frontend = Fixture::start(false).await;
        let unconfigured = Fixture::start(false).await;
        let config = WebProxyConfig {
            upstream: api.addr,
            passthrough: vec![frontend.addr],
        };
        let proxy = TestProxy::start(config.clone()).await.unwrap();
        let other = TestProxy::start(config).await.unwrap();
        assert_ne!(proxy.address(), other.address());
        assert!(
            get(&proxy, api.addr, "/api?keep=1")
                .await
                .ends_with("/api?keep=1")
        );
        let fault = WebFault::HttpStatus {
            path: "/api".into(),
            status: 503,
        };
        assert_eq!(proxy.set(Some(fault)).unwrap().active.hits, 0);
        let blocked = get(&proxy, api.addr, "/api?keep=2").await;
        assert!(blocked.starts_with("HTTP/1.1 503"));
        assert!(
            blocked
                .to_lowercase()
                .contains("access-control-allow-origin: http://fixture.test")
        );
        assert_eq!(api.requests(), 1);
        assert!(
            get(&proxy, api.addr, "/other")
                .await
                .starts_with("HTTP/1.1 200")
        );
        assert!(
            get(&proxy, frontend.addr, "/api")
                .await
                .starts_with("HTTP/1.1 200")
        );
        assert!(
            get(&proxy, unconfigured.addr, "/api")
                .await
                .starts_with("HTTP/1.1 403")
        );
        assert_eq!(unconfigured.requests(), 0);
        assert!(
            get(&other, api.addr, "/api")
                .await
                .starts_with("HTTP/1.1 200")
        );
        assert_eq!(proxy.state().hits, 1);
        assert_eq!(other.state(), WebFaultState::default());
        assert_eq!(proxy.set(None).unwrap().previous.hits, 1);
        assert!(
            get(&proxy, api.addr, "/api")
                .await
                .starts_with("HTTP/1.1 200")
        );
    }

    #[tokio::test]
    async fn delayed_and_dropped_http_are_distinct_from_a_mock_response() {
        let api = Fixture::start(false).await;
        let proxy = TestProxy::start(WebProxyConfig {
            upstream: api.addr,
            passthrough: vec![],
        })
        .await
        .unwrap();
        proxy
            .set(Some(WebFault::Delay {
                path: "/data".into(),
                ms: 60,
            }))
            .unwrap();
        let start = tokio::time::Instant::now();
        assert!(
            get(&proxy, api.addr, "/data")
                .await
                .starts_with("HTTP/1.1 200")
        );
        assert!(start.elapsed() >= Duration::from_millis(60));
        assert_eq!(proxy.state().hits, 1);
        proxy
            .set(Some(WebFault::Drop {
                path: "/data".into(),
            }))
            .unwrap();
        assert_eq!(get(&proxy, api.addr, "/data").await, "");
        assert_eq!(proxy.state().hits, 1);
        assert_eq!(api.requests(), 1);
    }

    async fn connect_through(
        proxy: &TestProxy,
        target: SocketAddr,
    ) -> (String, BufReader<TcpStream>) {
        let mut socket = BufReader::new(TcpStream::connect(proxy.address()).await.unwrap());
        socket
            .write_all(format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let head = timeout(Duration::from_secs(2), header_block(&mut socket))
            .await
            .unwrap()
            .unwrap();
        (head, socket)
    }

    #[tokio::test]
    async fn established_test_tunnels_can_be_cut_and_recovered() {
        let api = Fixture::start(true).await;
        let proxy = TestProxy::start(WebProxyConfig {
            upstream: api.addr,
            passthrough: vec![],
        })
        .await
        .unwrap();
        proxy.set(Some(WebFault::WebSocketReject)).unwrap();
        assert!(
            connect_through(&proxy, api.addr)
                .await
                .0
                .starts_with("HTTP/1.1 503")
        );
        assert_eq!(proxy.set(None).unwrap().previous.hits, 1);
        let (head, mut tunnel) = connect_through(&proxy, api.addr).await;
        assert!(head.starts_with("HTTP/1.1 200"));
        tunnel.write_all(b"ping").await.unwrap();
        let mut message = [0; 4];
        timeout(Duration::from_secs(2), tunnel.read_exact(&mut message))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&message, b"ping");
        assert_eq!(
            proxy
                .set(Some(WebFault::WebSocketDrop))
                .unwrap()
                .active
                .hits,
            1
        );
        let closed = timeout(Duration::from_secs(2), tunnel.read(&mut message))
            .await
            .unwrap();
        assert!(matches!(closed, Ok(0)) || closed.is_err());
        proxy.set(None).unwrap();
        assert!(
            connect_through(&proxy, api.addr)
                .await
                .0
                .starts_with("HTTP/1.1 200")
        );
    }

    #[tokio::test]
    async fn forked_driver_has_no_proxy_and_close_drops_owned_listener() {
        use robot_core::Robot;
        let api = Fixture::start(false).await;
        let robot = crate::ChromeRobot::new("127.0.0.1:0");
        let config = WebProxyConfig {
            upstream: api.addr,
            passthrough: vec![],
        };
        robot
            .configure_web_proxy("chrome:browser", config.clone())
            .await
            .unwrap();
        let address = robot.test_proxy.lock().await.as_ref().unwrap().address();
        assert!(
            robot
                .configure_web_proxy("chrome:browser", config)
                .await
                .is_err()
        );
        assert!(
            robot
                .fork_session()
                .unwrap()
                .web_fault_state("chrome:browser")
                .await
                .is_err()
        );
        assert!(robot.restart_device("chrome:browser").await.is_err());
        robot.close_session().await.unwrap();
        timeout(Duration::from_secs(2), async {
            while TcpStream::connect(address).await.is_ok() {
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }
}
