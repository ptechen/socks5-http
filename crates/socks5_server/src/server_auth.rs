use async_trait::async_trait;
use tokio::net::TcpStream;
use socks5_protocol::{AsyncStreamOperation, AuthMethod, UserKey};
use socks5_protocol::password_method::{Request, Response};
use socks5_protocol::password_method::Status::{Failed, Succeeded};
use crate::AuthExecutor;
use error::{Error, Result};

pub struct ServerAuth {
    is_white: bool,
}

#[async_trait]
impl AuthExecutor for ServerAuth {
    type Output = Result<bool>;

    fn auth_method(&self) -> AuthMethod {
        if self.is_white {
            AuthMethod::NoAuth
        } else {
            AuthMethod::UserPass
        }
    }

    async fn execute(&self, stream: &mut TcpStream) -> Self::Output {
        match self.auth_method() {
            AuthMethod::NoAuth => {
                Ok(true)
            }
            AuthMethod::UserPass => {
                let req = Request::retrieve_from_async_stream(stream).await?;
                //  todo!()
                let resp = Response::new(if is_equal { Succeeded } else { Failed });
                resp.write_to_async_stream(stream).await?;
                if is_equal {
                    Ok(true)
                } else {
                    Err(Error::from(std::io::Error::new(std::io::ErrorKind::Other, "username or password is incorrect")))
                }
            }
            _ => Ok(false)
        }
    }
}