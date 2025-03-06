use axum::body::Body;
use axum::extract::Request;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use hyper::upgrade::Upgraded;
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;
use tracing::info;

pub struct HttpProxy;

impl HttpProxy {
    pub async fn proxy(req: Request, _remote_ip: &str) -> Result<Response, hyper::Error> {
        if let Some(host_addr) = req.uri().authority().map(|auth| auth.to_string()) {
            tokio::task::spawn(async move {
                match hyper::upgrade::on(req).await {
                    Ok(upgraded) => {
                        if let Err(e) = Self::tunnel(upgraded, host_addr).await {
                            tracing::warn!("server io error: {}", e);
                        };
                    }
                    Err(e) => tracing::warn!("upgrade error: {}", e),
                }
            });

            Ok(Response::new(Body::empty()))
        } else {
            tracing::warn!("CONNECT host is not socket addr: {:?}", req.uri());
            Ok((StatusCode::BAD_REQUEST, "CONNECT must be to a socket address").into_response())
        }
    }

    async fn tunnel(upgraded: Upgraded, addr: String) -> std::io::Result<()> {
        let mut server = TcpStream::connect(addr).await?;
        let mut upgraded = TokioIo::new(upgraded);

        let (from_client, from_server) = tokio::io::copy_bidirectional(&mut upgraded, &mut server).await?;

        tracing::debug!("client wrote {} bytes and received {} bytes", from_client, from_server);

        Ok(())
    }

    pub fn basic_auth(req: &Request<Body>) -> Result<(), Response> {
        const USERNAME: &str = "username";
        const PASSWORD: &str = "password";
        if let Some(auth) = req.headers().get(header::PROXY_AUTHORIZATION) {
            if let Ok(auth) = auth.to_str() {
                if let Some(auth) = auth.strip_prefix("Basic ") {
                    if let Ok(decoded) = base64::prelude::BASE64_STANDARD.decode(auth) {
                        if let Ok(decoded) = String::from_utf8(decoded) {
                            if let Some((username, password)) = decoded.split_once(':') {
                                info!("username: {}, password: {}", username, password);
                                return if username == USERNAME && password == PASSWORD {
                                    Ok(())
                                } else {
                                    let response = Response::builder()
                                        .status(StatusCode::UNAUTHORIZED)
                                        .header(header::WWW_AUTHENTICATE, r#"Basic realm="proxy""#)
                                        .body(Body::from("Unauthorized"))
                                        .unwrap();
                                    Err(response)
                                }
                            }
                        }
                    }
                }
            }
        }

        let response = Response::builder()
            .status(StatusCode::PROXY_AUTHENTICATION_REQUIRED)
            .header(header::WWW_AUTHENTICATE, r#"Basic realm="proxy""#)
            .body(Body::from("Proxy Authentication Required"))
            .unwrap();
        Err(response)
    }
}
