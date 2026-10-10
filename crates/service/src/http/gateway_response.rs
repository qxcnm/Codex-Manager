//! Gateway response values independent of any server implementation.
//! Hyper serializes and frames these values at the Axum boundary.
use axum::http::{HeaderName, HeaderValue};
use std::fmt;
#[cfg(test)]
use std::io::Read;
use std::io::{self, Cursor};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HeaderField(HeaderName);
impl HeaderField {
    pub(crate) fn as_str(&self) -> &str {
        self.0.as_str()
    }
    pub(crate) fn equiv(&self, name: &str) -> bool {
        self.0.as_str().eq_ignore_ascii_case(name)
    }
}
impl fmt::Display for HeaderField {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Header {
    pub(crate) field: HeaderField,
    pub(crate) value: String,
}
impl Header {
    pub(crate) fn from_bytes(name: impl AsRef<[u8]>, value: impl AsRef<[u8]>) -> io::Result<Self> {
        let field = HeaderName::from_bytes(name.as_ref()).map_err(io::Error::other)?;
        HeaderValue::from_bytes(value.as_ref()).map_err(io::Error::other)?;
        let value = String::from_utf8(value.as_ref().to_vec()).map_err(io::Error::other)?;
        Ok(Self {
            field: HeaderField(field),
            value,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StatusCode(pub(crate) u16);
impl From<u16> for StatusCode {
    fn from(value: u16) -> Self {
        Self(value)
    }
}
#[cfg(test)]
impl StatusCode {
    pub(crate) fn default_reason_phrase(&self) -> &'static str {
        axum::http::StatusCode::from_u16(self.0)
            .ok()
            .and_then(|status| status.canonical_reason())
            .unwrap_or("")
    }
}

pub(crate) struct Response<R> {
    status: StatusCode,
    headers: Vec<Header>,
    reader: R,
    length: Option<usize>,
}
impl<R> Response<R> {
    pub(crate) fn new(
        status: StatusCode,
        headers: Vec<Header>,
        reader: R,
        length: Option<usize>,
    ) -> Self {
        let mut response = Self {
            status,
            headers: Vec::new(),
            reader,
            length,
        };
        for header in headers {
            response.add_header(header);
        }
        response
    }
    pub(crate) fn status_code(&self) -> StatusCode {
        self.status
    }
    pub(crate) fn headers(&self) -> &[Header] {
        &self.headers
    }
    pub(crate) fn data_length(&self) -> Option<usize> {
        self.length
    }
    pub(crate) fn into_reader(self) -> R {
        self.reader
    }
    #[cfg(test)]
    pub(crate) fn into_parts(self) -> (StatusCode, Vec<Header>, R, Option<usize>) {
        (self.status, self.headers, self.reader, self.length)
    }
    pub(crate) fn add_header(&mut self, header: Header) {
        if header.field.equiv("content-length") {
            if let Ok(length) = header.value.parse() {
                self.length = Some(length);
            }
            return;
        }
        if super::header_filter::should_skip_response_header(&header.field.0) {
            return;
        }
        if header.field.equiv("content-type") {
            self.headers
                .retain(|existing| !existing.field.equiv("content-type"));
        }
        self.headers.push(header);
    }
    pub(crate) fn with_header(mut self, header: Header) -> Self {
        self.add_header(header);
        self
    }
    pub(crate) fn with_status_code(mut self, status: impl Into<StatusCode>) -> Self {
        self.status = status.into();
        self
    }
    #[cfg(test)]
    pub(crate) fn into_test_response(self) -> tiny_http::Response<R>
    where
        R: Read,
    {
        let headers = self
            .headers
            .into_iter()
            .map(|header| {
                tiny_http::Header::from_bytes(header.field.as_str(), header.value)
                    .expect("validated test header")
            })
            .collect();
        tiny_http::Response::new(
            tiny_http::StatusCode(self.status.0),
            headers,
            self.reader,
            self.length,
            None,
        )
    }
}
impl Response<Cursor<Vec<u8>>> {
    pub(crate) fn from_string(value: impl Into<String>) -> Self {
        let value = value.into().into_bytes();
        let length = value.len();
        Self::new(
            StatusCode(200),
            vec![
                Header::from_bytes("content-type", "text/plain; charset=UTF-8")
                    .expect("static header"),
            ],
            Cursor::new(value),
            Some(length),
        )
    }
}
impl Response<io::Empty> {
    pub(crate) fn empty(status: impl Into<StatusCode>) -> Self {
        Self::new(status.into(), Vec::new(), io::empty(), Some(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn headers_reject_injection_and_preserve_content_type_replacement() {
        assert!(Header::from_bytes("x-safe", "value\r\nx-injected: true").is_err());
        assert!(Header::from_bytes("invalid name", "value").is_err());
        let response = Response::from_string("{}")
            .with_header(Header::from_bytes("Content-Type", "application/json").unwrap())
            .with_header(Header::from_bytes("Connection", "keep-alive").unwrap());
        assert_eq!(response.headers.len(), 1);
        assert_eq!(response.headers[0].value, "application/json");
        assert_eq!(response.length, Some(2));
    }
}
