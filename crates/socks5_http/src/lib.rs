use error::Result;
use hyper_util::rt::TokioIo;
use socks5_protocol::Version;
use std::io::Read;
use std::net::TcpStream;
pub struct Sock5Http {
    pub sock5_or_http: TokioIo<TcpStream>,
}

pub enum Sock5OrHttp {
    Sock5,
    Http,
}

impl Sock5Http {
    pub fn new(sock5_or_http: TcpStream) -> Self {
        Self {
            sock5_or_http: TokioIo::new(sock5_or_http),
        }
    }

    async fn socks5_or_http(&self) -> Result<Sock5OrHttp> {
        let mut ver = [0u8; 1];
        self.sock5_or_http.inner().read(&mut ver)?;
        let version = Version::try_from(ver[0])?;
        if version == Version::V5 {
            Ok(Sock5OrHttp::Sock5)
        } else {
            Ok(Sock5OrHttp::Http)
        }
    }
}
