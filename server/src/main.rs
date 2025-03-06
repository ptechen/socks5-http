use axum::Router;
use axum::body::Body;
use axum::extract::Request;
use axum::http::Method;
use axum::routing::get;
use error::Result;
use http_impl::proxy::HttpProxy;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use socks5_server::auth::NoAuth;
use socks5_server::handle_stream::handle_stream;
use std::{net::SocketAddr, sync::Arc};
use tokio::net::TcpListener;
use tokio::signal;
use tower::{Service, ServiceExt};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;
use socks5_http::{Sock5Http, Sock5OrHttp};

#[tokio::main]
async fn main() -> Result<()> {
    // a builder for `FmtSubscriber`.
    let subscriber = FmtSubscriber::builder()
        // all spans/events with a level higher than TRACE (e.g, debug, info, warn, etc.)
        // will be written to stdout.
        .with_max_level(Level::TRACE)
        // completes the builder.
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");

    let router_svc = Router::new().route("/", get(|| async { "Request Failed" }));
    let router_svc1 = router_svc.clone();
    let router_svc2 = router_svc.clone();
    let tower_service = tower::service_fn(move |req: Request<_>| {
        let router_svc = router_svc1.clone();
        let req = req.map(Body::new);
        async move {
            if req.method() == Method::CONNECT {
                if let Err(response) = HttpProxy::basic_auth(&req) {
                    return Ok(response)
                }
                HttpProxy::proxy(req).await
            } else {
                if let Err(response) = HttpProxy::basic_auth(&req) {
                    return Ok(response)
                }
                router_svc.oneshot(req).await.map_err(|err| match err {})
            }
        }
    });

    let tower_service_no_auth = tower::service_fn(move |req: Request<_>| {
        let router_svc = router_svc2.clone();
        let req = req.map(Body::new);
        async move {
            if req.method() == Method::CONNECT {
                HttpProxy::proxy(req).await
            } else {
                router_svc.oneshot(req).await.map_err(|err| match err {})
            }
        }
    });

    let hyper_service = hyper::service::service_fn(move |request: Request<Incoming>| tower_service.clone().call(request));
    let hyper_service_no_auth = hyper::service::service_fn(move |request: Request<Incoming>| tower_service_no_auth.clone().call(request));

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    tracing::debug!("listening on {}", addr);

    let listener = TcpListener::bind(addr).await?;
    'tag: loop {
        tokio::select! {
            Ok((stream, socket_addr)) = listener.accept() => {
                //todo is white
                let mut is_white = false;
                tracing::debug!("accepted connection from {}", socket_addr);
                let socks5_or_http = Sock5Http::socks5_or_http(&stream).await?;
                match socks5_or_http {
                    Sock5OrHttp::Socks => {
                        let auth = NoAuth;
                        let auth = Arc::new(auth);
                        if let Err(err) = handle_stream(stream, auth).await {
                            tracing::error!("{}", err);
                        }
                    }
                    Sock5OrHttp::Http => {
                        tracing::debug!("not socks5 protocol: {:?}", stream.peer_addr());
                        let io = TokioIo::new(stream);
                        if is_white {
                            let hyper_service = hyper_service_no_auth.clone();
                            tokio::task::spawn(async move {
                            if let Err(err) = http1::Builder::new()
                                .preserve_header_case(true)
                                .title_case_headers(true)
                                .serve_connection(io, hyper_service)
                                .with_upgrades()
                                .await
                                {
                                    tracing::error!("Failed to serve connection: {:?}", err);
                                }
                            });
                        } else {
                            let hyper_service = hyper_service.clone();
                            tokio::task::spawn(async move {
                                if let Err(err) = http1::Builder::new()
                                    .preserve_header_case(true)
                                    .title_case_headers(true)
                                    .serve_connection(io, hyper_service)
                                    .with_upgrades()
                                    .await
                                {
                                    tracing::error!("Failed to serve connection: {:?}", err);
                                }
                            });
                        }
                    }
                }
            },
            _ = shutdown_signal() => {
                break 'tag;
            },
        }
    }
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c().await.expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
