use std::future::Future;

use bytes::Bytes;
use http::{HeaderValue, Request, Response, header};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper_util::{
    client::legacy::{
        Client,
        connect::{Connect, HttpConnector},
    },
    rt::TokioExecutor,
};
use hyperlocal::{UnixConnector, Uri};

use crate::error::TransportError;

pub enum TransportKind {
    Tcp(TcpTransport),
    Unix(UnixTransport),
}

pub trait Transport: Send + Sync {
    fn send(
        &self,
        req: Request<Bytes>,
    ) -> impl Future<Output = Result<Response<Bytes>, TransportError>> + Send;
}

impl Transport for TransportKind {
    async fn send(&self, req: Request<Bytes>) -> Result<Response<Bytes>, TransportError> {
        match self {
            Self::Tcp(t) => t.send(req).await,
            Self::Unix(t) => t.send(req).await,
        }
    }
}

async fn execute<C>(
    client: &Client<C, Full<Bytes>>,
    req: Request<Bytes>,
) -> Result<Response<Bytes>, TransportError>
where
    C: Connect + Clone + Send + Sync + 'static,
{
    let (parts, body) = req.into_parts();
    let resp: Response<Incoming> = client
        .request(Request::from_parts(parts, Full::new(body)))
        .await?;

    let (parts, body) = resp.into_parts();
    Ok(Response::from_parts(
        parts,
        body.collect().await?.to_bytes(),
    ))
}

// --- TCP ---

pub struct TcpTransport {
    client: Client<HttpConnector, Full<Bytes>>,
}

impl TcpTransport {
    pub fn new() -> Self {
        Self {
            client: Client::builder(TokioExecutor::new()).build(HttpConnector::new()),
        }
    }
}

impl Default for TcpTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl Transport for TcpTransport {
    async fn send(&self, req: Request<Bytes>) -> Result<Response<Bytes>, TransportError> {
        execute(&self.client, req).await
    }
}

// --- Unix ---

pub struct UnixTransport {
    client: Client<UnixConnector, Full<Bytes>>,
    socket_path: String,
}

impl UnixTransport {
    pub fn new(socket_path: impl Into<String>) -> Self {
        Self {
            client: Client::builder(TokioExecutor::new()).build(UnixConnector),
            socket_path: socket_path.into(),
        }
    }
}

impl Transport for UnixTransport {
    async fn send(&self, req: Request<Bytes>) -> Result<Response<Bytes>, TransportError> {
        let (mut parts, body) = req.into_parts();

        let path_and_query = parts.uri.path_and_query().map_or("/", |pq| pq.as_str());

        parts
            .headers
            .entry(header::HOST)
            .or_insert(HeaderValue::from_static("localhost"));

        parts.uri = Uri::new(&self.socket_path, path_and_query).into();

        execute(&self.client, Request::from_parts(parts, body)).await
    }
}
