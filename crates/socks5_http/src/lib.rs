use error::Result;
use tokio::net::TcpStream;
use socks5_protocol::Version;
pub struct SocksHttp;

pub enum SocksOrHttp {
    Socks,
    Http,
}

impl SocksHttp {
    pub async fn socks_or_http(stream: &TcpStream) -> Result<SocksOrHttp> {
        let mut ver = [0u8; 1];
        stream.peek(&mut ver).await?;
        let socks5_or_http = match Version::try_from(ver[0]) {
            Ok(version) => {
                if version == Version::V5 || version == Version::V4 {
                    SocksOrHttp::Socks
                } else {
                    SocksOrHttp::Http
                }
            },
            Err(_) => {
                SocksOrHttp::Http
            }
        };
        Ok(socks5_or_http)
    }
}
