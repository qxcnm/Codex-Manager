use super::{DisconnectTrackingBody, ResponseBodyDeliveryState};
use crate::http::gateway_request::GatewayRequest;
use crate::http::gateway_response_body::{BodyReadFuture, GatewayResponseBody};
use axum::body::Body;
use bytes::Bytes;
use http_body::Body as HttpBody;
use std::io::{self, Cursor, Read};
use std::pin::Pin;
use std::time::Duration;
use tokio::sync::oneshot;

struct TerminalReader {
    bytes: Cursor<Vec<u8>>,
    terminal_read: Option<oneshot::Sender<()>>,
}

impl GatewayResponseBody for TerminalReader {
    fn read_async<'a>(&'a mut self, buffer: &'a mut [u8]) -> BodyReadFuture<'a> {
        Box::pin(async move {
            assert!(!self.is_end_stream(), "no provider read after completion");
            let count = self.bytes.read(buffer)?;
            if self.is_end_stream() {
                if let Some(read) = self.terminal_read.take() {
                    let _ = read.send(());
                }
            }
            Ok(count)
        })
    }

    fn is_end_stream(&self) -> bool {
        self.bytes.position() == self.bytes.get_ref().len() as u64
    }
}

async fn native_fixture(
    bytes: Vec<u8>,
) -> (
    DisconnectTrackingBody,
    oneshot::Receiver<()>,
    tokio::task::JoinHandle<io::Result<()>>,
) {
    let (parts, ()) = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/responses")
        .body(())
        .unwrap()
        .into_parts();
    let (request, response) = GatewayRequest::new(parts, Bytes::new());
    let (terminal_read, read_completed) = oneshot::channel();
    let worker = tokio::spawn(
        request.respond_async(crate::http::gateway_response::Response::new(
            crate::http::gateway_response::StatusCode(200),
            vec![],
            TerminalReader {
                bytes: Cursor::new(bytes),
                terminal_read: Some(terminal_read),
            },
            None,
        )),
    );
    let (parts, body) = response.await.unwrap().into_parts();
    let delivery_state = parts.extensions.get::<ResponseBodyDeliveryState>().cloned();
    assert!(
        delivery_state.is_some(),
        "native responses expose shared state"
    );
    (
        DisconnectTrackingBody {
            body,
            path: "/v1/responses".to_owned(),
            completed: false,
            delivery_state,
        },
        read_completed,
        worker,
    )
}

async fn next_bytes(body: &mut DisconnectTrackingBody) -> Bytes {
    tokio::time::timeout(
        Duration::from_secs(2),
        futures_util::future::poll_fn(|cx| Pin::new(&mut *body).poll_frame(cx)),
    )
    .await
    .unwrap()
    .unwrap()
    .unwrap()
    .into_data()
    .unwrap()
}

#[tokio::test]
async fn consumed_native_terminal_does_not_record_front_proxy_499_before_http_eof() {
    let wire = b"event: response.completed\n\n".to_vec();
    let (mut body, terminal_read, worker) = native_fixture(wire.clone()).await;
    terminal_read.await.unwrap();
    assert!(
        body.disconnected_before_completion(),
        "queueing cannot suppress 499"
    );
    assert_eq!(next_bytes(&mut body).await.as_ref(), wire);
    assert!(!body.completed, "HTTP EOF has not been polled");
    assert!(body.delivery_state.as_ref().unwrap().terminal_consumed());
    assert!(
        !body.disconnected_before_completion(),
        "the consumed terminal must suppress the outer false disconnect count"
    );
    drop(body);
    tokio::time::timeout(Duration::from_secs(2), worker)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn unconsumed_native_terminal_still_records_front_proxy_disconnect() {
    let (body, terminal_read, worker) =
        native_fixture(b"event: response.completed\n\n".to_vec()).await;
    terminal_read.await.unwrap();
    assert!(!body.delivery_state.as_ref().unwrap().terminal_consumed());
    assert!(body.disconnected_before_completion());
    drop(body);
    let error = tokio::time::timeout(Duration::from_secs(2), worker)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
}

#[tokio::test]
async fn partial_native_terminal_still_records_front_proxy_disconnect() {
    let (mut body, terminal_read, worker) = native_fixture(vec![b'x'; 32799]).await;
    terminal_read.await.unwrap();
    assert_eq!(next_bytes(&mut body).await.len(), 16 * 1024);
    assert!(!body.delivery_state.as_ref().unwrap().terminal_consumed());
    assert!(body.disconnected_before_completion());
    drop(body);
    let error = tokio::time::timeout(Duration::from_secs(2), worker)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
}

#[tokio::test]
async fn proxy_without_native_terminal_state_keeps_http_eof_semantics() {
    let mut body = DisconnectTrackingBody {
        body: Body::from("event: response.completed\n\n"),
        path: "/v1/responses".to_owned(),
        completed: false,
        delivery_state: None,
    };
    assert!(!next_bytes(&mut body).await.is_empty());
    assert!(
        body.disconnected_before_completion(),
        "terminal-looking payload bytes alone are not consumption evidence"
    );
    assert!(
        futures_util::future::poll_fn(|cx| Pin::new(&mut body).poll_frame(cx))
            .await
            .is_none()
    );
    assert!(!body.disconnected_before_completion());
}
