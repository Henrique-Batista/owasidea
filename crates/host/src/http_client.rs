//! HTTP/1.1 client that connects to a pinned address while keeping the
//! original hostname for TLS SNI and certificate validation.

use bytes::Bytes;
use core::future::poll_fn;
use core::pin::{pin, Pin};
use core::task::{ready, Context, Poll};
use http::uri::Scheme;
use http::{Request, Response};
use hyper_util::rt::TokioIo;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use wasmtime_wasi_http::{Error, RequestOptions, WasiBody};

trait TokioStream: AsyncRead + AsyncWrite + Send + Sync + Unpin + 'static {
    fn boxed(self) -> Box<dyn TokioStream>
    where
        Self: Sized,
    {
        Box::new(self)
    }
}
impl<T> TokioStream for T where T: AsyncRead + AsyncWrite + Send + Sync + Unpin + 'static {}

fn connect_error(message: impl std::fmt::Display) -> Error {
    Error::Connect(std::io::Error::other(message.to_string()))
}

fn tls_name(host: &str) -> Result<rustls::pki_types::ServerName<'static>, Error> {
    use rustls::pki_types::ServerName;
    if let Ok(ip) = host.trim_matches(['[', ']']).parse::<IpAddr>() {
        return Ok(ServerName::from(ip));
    }
    ServerName::try_from(host.to_string())
        .map_err(|error| Error::Tls(std::io::Error::other(error.to_string())))
}

pub(crate) async fn send_pinned(
    mut request: Request<WasiBody>,
    options: Option<RequestOptions>,
    pinned: Vec<IpAddr>,
) -> Result<
    (
        Response<WasiBody>,
        Box<dyn std::future::Future<Output = Result<(), Error>> + Send>,
    ),
    Error,
> {
    let uri = request.uri();
    let authority = uri.authority().ok_or(Error::HttpRequestUriInvalid)?;
    let host = authority.host().to_string();
    let use_tls = uri.scheme() == Some(&Scheme::HTTPS);
    let port = authority
        .port_u16()
        .unwrap_or(if use_tls { 443 } else { 80 });
    if pinned.is_empty() {
        return Err(connect_error("no pinned address for HTTP host"));
    }

    let connect_timeout = options
        .as_ref()
        .and_then(|options| options.connect_timeout)
        .unwrap_or(Duration::from_secs(30));
    let first_byte_timeout = options
        .as_ref()
        .and_then(|options| options.first_byte_timeout)
        .unwrap_or(Duration::from_secs(30));
    let between_bytes_timeout = options
        .as_ref()
        .and_then(|options| options.between_bytes_timeout)
        .unwrap_or(Duration::from_secs(30));

    let mut last_error = None;
    let mut stream = None;
    for ip in pinned {
        let address = SocketAddr::new(ip, port);
        match tokio::time::timeout(connect_timeout, TcpStream::connect(address)).await {
            Ok(Ok(connected)) => {
                stream = Some(connected);
                break;
            }
            Ok(Err(error)) => last_error = Some(error.to_string()),
            Err(_) => last_error = Some("connection timed out".to_string()),
        }
    }
    let stream = stream.ok_or_else(|| {
        connect_error(last_error.unwrap_or_else(|| "no pinned address reachable".to_string()))
    })?;

    let stream: Box<dyn TokioStream> = if use_tls {
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.into(),
        };
        let config = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let connector = tokio_rustls::TlsConnector::from(std::sync::Arc::new(config));
        let stream = connector
            .connect(tls_name(&host)?, stream)
            .await
            .map_err(Error::Tls)?;
        stream.boxed()
    } else {
        stream.boxed()
    };

    let (mut sender, connection) = tokio::time::timeout(
        connect_timeout,
        hyper::client::conn::http1::Builder::new().handshake(TokioIo::new(stream)),
    )
    .await
    .map_err(|_| Error::ConnectionTimeout)??;

    *request.uri_mut() = http::Uri::builder()
        .path_and_query(
            request
                .uri()
                .path_and_query()
                .map(|path| path.as_str())
                .unwrap_or("/"),
        )
        .build()
        .expect("valid request URI");

    struct IncomingBody {
        incoming: hyper::body::Incoming,
        timeout: tokio::time::Interval,
    }

    impl http_body::Body for IncomingBody {
        type Data = Bytes;
        type Error = Error;

        fn poll_frame(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
            match Pin::new(&mut self.as_mut().incoming).poll_frame(cx) {
                Poll::Ready(None) => Poll::Ready(None),
                Poll::Ready(Some(Err(error))) => Poll::Ready(Some(Err(if error.is_timeout() {
                    Error::HttpResponseTimeout
                } else {
                    Error::from(error)
                }))),
                Poll::Ready(Some(Ok(frame))) => {
                    self.timeout.reset();
                    Poll::Ready(Some(Ok(frame)))
                }
                Poll::Pending => {
                    ready!(self.timeout.poll_tick(cx));
                    Poll::Ready(Some(Err(Error::ConnectionReadTimeout)))
                }
            }
        }

        fn is_end_stream(&self) -> bool {
            self.incoming.is_end_stream()
        }

        fn size_hint(&self) -> http_body::SizeHint {
            self.incoming.size_hint()
        }
    }

    let send = async move {
        match tokio::time::timeout(first_byte_timeout, sender.send_request(request)).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(error)) => Err(Error::from(error)),
            Err(_) => Err(Error::ConnectionReadTimeout),
        }
    };
    let mut send = pin!(send);
    let mut connection = Some(connection);
    let response = poll_fn(|cx| match send.as_mut().poll(cx) {
        Poll::Ready(response) => Poll::Ready(response),
        Poll::Pending => {
            let Some(future) = connection.as_mut() else {
                return Poll::Pending;
            };
            let result = ready!(Pin::new(future).poll(cx));
            connection = None;
            match result {
                Ok(()) => send.as_mut().poll(cx),
                Err(error) => Poll::Ready(Err(Error::from(error))),
            }
        }
    })
    .await?;

    let between_bytes_timeout = if between_bytes_timeout == Duration::ZERO {
        Duration::new(0, 1)
    } else {
        between_bytes_timeout
    };
    let mut timeout = tokio::time::interval(between_bytes_timeout);
    timeout.reset();
    let response = response.map(|incoming| {
        use http_body_util::BodyExt;
        IncomingBody { incoming, timeout }.boxed_unsync()
    });

    Ok((
        response,
        Box::new(async move {
            let Some(connection) = connection else {
                return Ok(());
            };
            connection.await.map_err(Error::from)?;
            Ok(())
        }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tls_name_keeps_dns_names_and_accepts_ip_literals() {
        assert!(matches!(
            tls_name("api.example.com").unwrap(),
            rustls::pki_types::ServerName::DnsName(_)
        ));
        assert!(matches!(
            tls_name("127.0.0.1").unwrap(),
            rustls::pki_types::ServerName::IpAddress(_)
        ));
    }

    #[tokio::test]
    async fn connects_to_pinned_ip_and_preserves_host_header() {
        use http_body_util::BodyExt;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = [0u8; 2048];
            let read = socket.read(&mut buffer).await.unwrap();
            let request = String::from_utf8_lossy(&buffer[..read]).to_ascii_lowercase();
            assert!(request.contains("host: pinned.test"));
            socket
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok")
                .await
                .unwrap();
        });

        let body = http_body_util::Full::new(Bytes::new())
            .map_err(|never| match never {})
            .boxed_unsync();
        let request = Request::builder()
            .uri(format!("http://pinned.test:{port}/health"))
            .header("host", "pinned.test")
            .body(body)
            .unwrap();
        let (response, _io) = send_pinned(request, None, vec!["127.0.0.1".parse().unwrap()])
            .await
            .expect("pinned request");
        assert_eq!(response.status(), 200);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"ok");
        server.await.unwrap();
    }
}
