//! Incremental gateway body input. Implementations await provider data and
//! perform only the current frame's parsing before returning bytes.
use std::future::Future;
use std::io::{self, Cursor, Read};
use std::pin::Pin;

pub(crate) type BodyReadFuture<'a> = Pin<Box<dyn Future<Output = io::Result<usize>> + Send + 'a>>;

pub(crate) trait GatewayResponseBody: Send {
    fn read_async<'a>(&'a mut self, buffer: &'a mut [u8]) -> BodyReadFuture<'a>;

    /// Finish observations already received before delivery was interrupted.
    /// Implementations must bound cleanup and must not wait for provider EOF.
    fn finish_async(&mut self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async {})
    }
}

impl<T: AsRef<[u8]> + Send> GatewayResponseBody for Cursor<T> {
    fn read_async<'a>(&'a mut self, buffer: &'a mut [u8]) -> BodyReadFuture<'a> {
        Box::pin(async move { self.read(buffer) })
    }
}

impl GatewayResponseBody for io::Empty {
    fn read_async<'a>(&'a mut self, _buffer: &'a mut [u8]) -> BodyReadFuture<'a> {
        Box::pin(async { Ok(0) })
    }
}

impl<T: GatewayResponseBody + ?Sized> GatewayResponseBody for Box<T> {
    fn read_async<'a>(&'a mut self, buffer: &'a mut [u8]) -> BodyReadFuture<'a> {
        (**self).read_async(buffer)
    }

    fn finish_async(&mut self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        (**self).finish_async()
    }
}

#[cfg(test)]
pub(crate) struct TestBodyReader<R>(pub(crate) R);

#[cfg(test)]
impl<R: GatewayResponseBody> Read for TestBodyReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        crate::gateway::response_test_runtime()?.block_on(self.0.read_async(buffer))
    }
}
