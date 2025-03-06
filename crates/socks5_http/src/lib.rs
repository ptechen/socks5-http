use error::Result;
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;
use socks5_protocol::Version;
pub struct Sock5Http;

pub enum Sock5OrHttp {
    Socks,
    Http,
}

impl Sock5Http {
    pub async fn socks5_or_http(stream: &TcpStream) -> Result<Sock5OrHttp> {
        let mut ver = [0u8; 1];
        stream.peek(&mut ver).await?;
        let socks5_or_http = match Version::try_from(ver[0]) {
            Ok(version) => {
                if version == Version::V5 || version == Version::V4 {
                    Sock5OrHttp::Socks
                } else {
                    Sock5OrHttp::Http
                }
            },
            Err(_) => {
                Sock5OrHttp::Http
            }
        };
        Ok(socks5_or_http)
    }
}
