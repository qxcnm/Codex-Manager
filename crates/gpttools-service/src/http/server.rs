use std::io;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use tiny_http::Request;
use tiny_http::Server;

const HTTP_WORKER_FACTOR: usize = 4;
const HTTP_WORKER_MIN: usize = 8;
const HTTP_QUEUE_FACTOR: usize = 4;
const HTTP_QUEUE_MIN: usize = 32;

fn http_worker_count() -> usize {
    // 中文注释：长流请求会占用处理线程；这里固定 worker 上限，避免并发时无限 spawn 拖垮进程。
    let cpus = thread::available_parallelism()
        .map(|v| v.get())
        .unwrap_or(4);
    (cpus * HTTP_WORKER_FACTOR).max(HTTP_WORKER_MIN)
}

fn http_queue_size(worker_count: usize) -> usize {
    // 中文注释：使用有界队列给入口施加背压；不这样做会在峰值流量下无限堆积请求并放大内存抖动。
    worker_count.saturating_mul(HTTP_QUEUE_FACTOR).max(HTTP_QUEUE_MIN)
}

fn spawn_request_workers(worker_count: usize, rx: mpsc::Receiver<Request>) {
    let shared_rx = Arc::new(Mutex::new(rx));
    for _ in 0..worker_count {
        let worker_rx = Arc::clone(&shared_rx);
        let _ = thread::spawn(move || loop {
            let request = {
                let Ok(guard) = worker_rx.lock() else {
                    break;
                };
                match guard.recv() {
                    Ok(request) => request,
                    Err(_) => break,
                }
            };
            route_request(request);
        });
    }
}

fn run_server(server: Server) {
    let worker_count = http_worker_count();
    let queue_size = http_queue_size(worker_count);
    let (tx, rx) = mpsc::sync_channel::<Request>(queue_size);
    spawn_request_workers(worker_count, rx);

    for request in server.incoming_requests() {
        if crate::shutdown_requested() || request.url() == "/__shutdown" {
            let _ = request.respond(tiny_http::Response::from_string("shutdown"));
            break;
        }
        if tx.send(request).is_err() {
            break;
        }
    }
}

pub fn start_http(addr: &str) -> std::io::Result<()> {
    // On Windows, "localhost" may resolve to IPv6 loopback only ([::1]), while some clients
    // prefer IPv4 (127.0.0.1). To keep using "localhost" in URLs and still support both
    // families, bind BOTH loopback listeners when the caller requests localhost.
    if let Some(port) = addr.strip_prefix("localhost:") {
        let v4 = Server::http(format!("127.0.0.1:{port}"))
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err));
        let v6 = Server::http(format!("[::1]:{port}"))
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err));

        match (v4, v6) {
            (Ok(v4_server), Ok(v6_server)) => {
                let join = thread::spawn(move || run_server(v6_server));
                run_server(v4_server);
                let _ = join.join();
                return Ok(());
            }
            (Ok(server), Err(_)) | (Err(_), Ok(server)) => {
                run_server(server);
                return Ok(());
            }
            (Err(err), Err(_)) => return Err(err),
        }
    }

    let server = Server::http(addr).map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
    run_server(server);
    Ok(())
}

pub fn route_request(request: Request) {
    let path = request.url().to_string();
    if request.method().as_str() == "POST" && path == "/rpc" {
        crate::http::rpc_endpoint::handle_rpc(request);
        return;
    }
    if request.method().as_str() == "GET" && path.starts_with("/auth/callback") {
        crate::http::callback_endpoint::handle_callback(request);
        return;
    }
    if request.method().as_str() == "GET" && path == "/metrics" {
        crate::http::gateway_endpoint::handle_metrics(request);
        return;
    }
    crate::http::gateway_endpoint::handle_gateway(request);
}
