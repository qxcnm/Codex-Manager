use std::io;
use std::thread;
use tiny_http::Request;
use tiny_http::Server;

pub fn start_http(addr: &str) -> std::io::Result<()> {
    let server = Server::http(addr).map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
    for request in server.incoming_requests() {
        if crate::shutdown_requested() || request.url() == "/__shutdown" {
            let _ = request.respond(tiny_http::Response::from_string("shutdown"));
            break;
        }
        thread::spawn(move || {
            route_request(request);
        });
    }
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
    crate::http::gateway_endpoint::handle_gateway(request);
}
