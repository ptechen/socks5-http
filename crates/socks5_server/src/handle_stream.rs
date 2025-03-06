use crate::connection::associate;
use crate::{AssociatedUdpSocket, AuthAdaptor, ClientConnection, IncomingConnection, UdpAssociate};
use as_any::AsAny;
use error::Error;
use socks5_protocol::{Address, Reply, UdpHeader};
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::Arc;
use tokio::io;
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::Mutex;

pub(crate) static MAX_UDP_RELAY_PACKET_SIZE: usize = 1500;
pub async fn handle_stream<S>(stream: TcpStream, auth: AuthAdaptor<S>) -> error::Result<()>
where
    S: Send + Sync + 'static,
{
    let conn = IncomingConnection::new(stream, auth);
    let (conn, res) = conn.authenticate().await?;
    if let Some(res) = res.as_any().downcast_ref::<std::io::Result<bool>>() {
        let res = *res.as_ref().map_err(|err| err.to_string())?;
        if !res {
            tracing::info!("authentication failed");
            return Ok(());
        }
    }

    match conn.wait_request().await? {
        ClientConnection::UdpAssociate(associate, _) => {
            handle_s5_upd_associate(associate).await?;
        }
        ClientConnection::Bind(bind, _) => {
            let mut conn = bind.reply(Reply::CommandNotSupported, Address::unspecified()).await?;
            conn.shutdown().await?;
        }
        ClientConnection::Connect(connect, addr) => {
            let target = match addr {
                Address::DomainAddress(domain, port) => TcpStream::connect((domain, port)).await,
                Address::SocketAddress(addr) => TcpStream::connect(addr).await,
            };

            if let Ok(mut target) = target {
                let mut conn = connect.reply(Reply::Succeeded, Address::unspecified()).await?;
                tracing::trace!("{} -> {}", conn.peer_addr()?, target.peer_addr()?);
                io::copy_bidirectional(&mut target, &mut conn).await?;
            } else {
                let mut conn = connect.reply(Reply::HostUnreachable, Address::unspecified()).await?;
                conn.shutdown().await?;
            }
        }
    }

    Ok(())
}

pub(crate) async fn handle_s5_upd_associate(associate: UdpAssociate<associate::NeedReply>) -> error::Result<()> {
    // listen on a random port
    let listen_ip = associate.local_addr()?.ip();
    let udp_listener = UdpSocket::bind(SocketAddr::from((listen_ip, 0))).await;

    match udp_listener.and_then(|socket| socket.local_addr().map(|addr| (socket, addr))) {
        Err(err) => {
            let mut conn = associate.reply(Reply::GeneralFailure, Address::unspecified()).await?;
            conn.shutdown().await?;
            Err(err.into())
        }
        Ok((listen_udp, listen_addr)) => {
            tracing::info!("[UDP] {listen_addr} listen on");

            let s5_listen_addr = Address::from(listen_addr);
            let mut reply_listener = associate.reply(Reply::Succeeded, s5_listen_addr).await?;

            let buf_size = MAX_UDP_RELAY_PACKET_SIZE - UdpHeader::max_serialized_len();
            let listen_udp = Arc::new(AssociatedUdpSocket::from((listen_udp, buf_size)));

            let zero_addr = SocketAddr::from(([0, 0, 0, 0], 0));

            let incoming_addr = Arc::new(Mutex::new(zero_addr));

            let dispatch_socket = UdpSocket::bind(zero_addr).await?;

            let res = loop {
                tokio::select! {
                    res = async {
                        let buf_size = MAX_UDP_RELAY_PACKET_SIZE - UdpHeader::max_serialized_len();
                        listen_udp.set_max_packet_size(buf_size);

                        let (pkt, frag, dst_addr, src_addr) = listen_udp.recv_from().await?;
                        if frag != 0 {
                            return Err("[UDP] packet fragment is not supported".into());
                        }

                        *incoming_addr.lock().await = src_addr;

                        tracing::trace!("[UDP] {src_addr} -> {dst_addr} incoming packet size {}", pkt.len());
                        let dst_addr = dst_addr.to_socket_addrs()?.next().ok_or("Invalid address")?;
                        dispatch_socket.send_to(&pkt, dst_addr).await?;
                        Ok::<_, Error>(())
                    } => {
                        if res.is_err() {
                            break res;
                        }
                    },
                    res = async {
                        let mut buf = vec![0u8; MAX_UDP_RELAY_PACKET_SIZE];
                        let (len, remote_addr) = dispatch_socket.recv_from(&mut buf).await?;
                        let incoming_addr = *incoming_addr.lock().await;
                        tracing::trace!("[UDP] {incoming_addr} <- {remote_addr} feedback to incoming");
                        listen_udp.send_to(&buf[..len], 0, remote_addr.into(), incoming_addr).await?;
                        Ok::<_, Error>(())
                    } => {
                        if res.is_err() {
                            break res;
                        }
                    },
                    _ = reply_listener.wait_until_closed() => {
                        tracing::trace!("[UDP] {} listener closed", listen_addr);
                        break Ok::<_, Error>(());
                    },
                };
            };

            reply_listener.shutdown().await?;

            res
        }
    }
}
