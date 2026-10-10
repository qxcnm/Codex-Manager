use axum::body::Body;
use axum::http::{request::Parts, Response, StatusCode};
use bytes::Bytes;

use super::gateway_request::GatewayRequest;
use super::proxy_response::text_error_response;

// Bound admitted routing tasks; network/header/gate waits release the executor.
static GATEWAY_DOMAIN_WORKERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(32);

/// A disconnected HTTP future may leave its routing worker running until the
/// upstream headers arrive. Drain those workers before deferred accounting so
/// none can register another continuation after listener shutdown has finished.
pub(crate) async fn drain_domain_workers() {
    let _workers = GATEWAY_DOMAIN_WORKERS.acquire_many(32).await;
}

pub(crate) async fn handle_gateway_http(parts: Parts, body: Bytes) -> Response<Body> {
    let permit = match GATEWAY_DOMAIN_WORKERS.try_acquire() {
        Ok(permit) => permit,
        Err(_) => return text_error_response(StatusCode::SERVICE_UNAVAILABLE, "service busy"),
    };
    let (mut request, receiver) = GatewayRequest::new(parts, body);
    if !crate::gateway::reserve_deferred_response(&mut request) {
        return text_error_response(StatusCode::SERVICE_UNAVAILABLE, "service busy");
    }
    let mut cancellation = request.cancellation_guard();
    let cancelled = request.cancellation_receiver();
    tokio::spawn(super::gateway_request::scope_response_cancellation(
        cancelled,
        async move {
            let _permit = permit;
            if request.is_cancelled() {
                return;
            }
            if crate::gateway::handle_gateway_request_async(request)
                .await
                .is_err()
            {
                log::error!("event=gateway_domain_request_failed");
            }
        },
    ));
    let response = receiver.await.unwrap_or_else(|_| {
        text_error_response(StatusCode::INTERNAL_SERVER_ERROR, "gateway request failed")
    });
    // From headers onward, the response body's guard owns cancellation.
    cancellation.disarm();
    response
}

#[cfg(test)]
pub(crate) fn handle_gateway(request: tiny_http::Request) {
    let _ = crate::gateway::handle_gateway_request(request.into());
}

#[cfg(test)]
pub(crate) fn handle_metrics(request: tiny_http::Request) {
    let response = tiny_http::Response::from_string(crate::gateway::gateway_metrics_prometheus())
        .with_header(
            tiny_http::Header::from_bytes(b"Content-Type", b"text/plain; version=0.0.4").unwrap(),
        );
    let _ = request.respond(response);
}
