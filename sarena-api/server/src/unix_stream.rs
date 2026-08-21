// unix_stream.rs (server side)
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::UnixStream,
};

/// Hyper calls `shutdown(SHUT_WR)` when finishing a connection. On Unix sockets
/// this returns ENOTCONN if the client already disconnected — which is benign.
/// This wrapper swallows that specific error so hyper doesn't log it as a failure.
pub struct UnixStreamCompat(pub UnixStream);

impl AsyncRead for UnixStreamCompat {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl AsyncWrite for UnixStreamCompat {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match Pin::new(&mut self.0).poll_shutdown(cx) {
            // Client already disconnected — this is normal for Unix sockets.
            Poll::Ready(Err(e)) if e.kind() == io::ErrorKind::NotConnected => Poll::Ready(Ok(())),
            other => other,
        }
    }
}
