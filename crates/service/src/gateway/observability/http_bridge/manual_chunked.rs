use crate::http::gateway_request::GatewayRequest as Request;
#[cfg(test)]
use std::io::{Read, Write};

use crate::http::gateway_response::{Header, Response, StatusCode};
#[cfg(test)]
use tiny_http::HTTPVersion;

#[cfg(test)]
const STREAMING_CHUNK_READ_BUF_BYTES: usize = 8 * 1024;

#[cfg(test)]
fn should_skip_streaming_manual_header(header: &Header) -> bool {
    header.field.equiv("connection")
        || header.field.equiv("content-length")
        || header.field.equiv("trailer")
        || header.field.equiv("transfer-encoding")
        || header.field.equiv("upgrade")
}

fn header_name_exists(headers: &[Header], name: &'static str) -> bool {
    headers.iter().any(|header| header.field.equiv(name))
}

#[cfg(test)]
pub(super) fn write_streaming_chunked_response<W, R>(
    writer: &mut W,
    http_version: &HTTPVersion,
    status: StatusCode,
    headers: &[Header],
    mut body: R,
    do_not_send_body: bool,
) -> std::io::Result<()>
where
    W: Write,
    R: Read,
{
    write!(
        writer,
        "HTTP/{}.{} {} {}\r\n",
        http_version.0,
        http_version.1,
        status.0,
        status.default_reason_phrase()
    )?;
    for header in headers {
        if should_skip_streaming_manual_header(header) {
            continue;
        }
        writer.write_all(header.field.as_str().as_bytes())?;
        writer.write_all(b": ")?;
        writer.write_all(header.value.as_str().as_bytes())?;
        writer.write_all(b"\r\n")?;
    }
    if !header_name_exists(headers, "x-accel-buffering") {
        writer.write_all(b"X-Accel-Buffering: no\r\n")?;
    }
    writer.write_all(b"Transfer-Encoding: chunked\r\n\r\n")?;
    writer.flush()?;

    if !do_not_send_body {
        let mut buffer = vec![0_u8; STREAMING_CHUNK_READ_BUF_BYTES];
        loop {
            let read = body.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            write!(writer, "{read:x}\r\n")?;
            writer.write_all(&buffer[..read])?;
            writer.write_all(b"\r\n")?;
            writer.flush()?;
        }
    }

    writer.write_all(b"0\r\n\r\n")?;
    writer.flush()
}

pub(super) async fn respond_streaming_chunked<R>(
    request: Request,
    status: StatusCode,
    mut headers: Vec<Header>,
    body: R,
) -> std::io::Result<()>
where
    R: crate::http::gateway_response_body::GatewayResponseBody + 'static,
{
    if !header_name_exists(&headers, "x-accel-buffering") {
        headers.push(Header::from_bytes(b"X-Accel-Buffering", b"no").expect("static header"));
    }
    // Hyper owns HTTP/1 chunk framing and HTTP/2 data framing. Keep the
    // incremental reader; do not write a nested HTTP response into the body.
    request
        .respond_async(Response::new(status, headers, body, None))
        .await
}
