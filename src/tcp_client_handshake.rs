use crate::error::Error;
use futures::try_ready;
use resol_vbus::BlobBuffer;
use tokio::net::TcpStream;
use tokio::prelude::*;

/// Handles the client-side of the VBus-over-TCP handshake.
///
/// # Examples
///
/// This example connects to a RESOL DL2 at "192.168.5.81", provides the
/// password for authorization and then send the DATA command. After that
/// the socket transmits raw VBus data and can be used with e.g.
/// `LiveDataStream`.
///
/// ```no_run
/// use tokio::prelude::*;
/// use tokio::net::TcpStream;
/// use tokio_resol_vbus::{Error, TcpClientHandshake};
///
/// let addr = "192.168.5.81:7053".parse().expect("Unable to parse address");
/// let handler = TcpStream::connect(&addr)
///     .map_err(Error::new)
///     .and_then(TcpClientHandshake::start)
///     .and_then(|hs| hs.send_pass_command("vbus"))
///     .and_then(|hs| hs.send_data_command())
///     .and_then(|socket| {
///         // use the socket
///         # Ok(())
///     })
///     .map_err(|err| eprintln!("{}", err));
/// tokio::run(handler);
/// ```
#[derive(Debug)]
pub struct TcpClientHandshake {
    socket: TcpStream,
    buf: BlobBuffer,
}

impl TcpClientHandshake {
    /// Start the VBus-over-TCP handshake as the client side connecting to a server.
    pub fn start(socket: TcpStream) -> impl Future<Item = Self, Error = Error> {
        let hs = TcpClientHandshake {
            socket,
            buf: BlobBuffer::new(),
        };

        hs.read_reply()
    }

    /// Consume `self` and return the underlying `TcpStream`.
    pub fn into_inner(self) -> TcpStream {
        self.socket
    }

    fn read_reply(self) -> impl Future<Item = Self, Error = Error> {
        let mut hs = Some(self);

        future::poll_fn(move || {
            let first_byte = loop {
                let hs = hs.as_mut().unwrap();

                if let Some(idx) = hs.buf.iter().position(|b| *b == 10) {
                    let first_byte = hs.buf[0];
                    hs.buf.consume(idx + 1);

                    break first_byte;
                }

                let mut buf = [0u8; 256];
                let len = try_ready!(hs.socket.poll_read(&mut buf));
                if len == 0 {
                    return Err(Error::new("Reached EOF"));
                }

                hs.buf.extend_from_slice(&buf[0..len]);
            };

            if first_byte == b'+' {
                Ok(Async::Ready(hs.take().unwrap()))
            } else if first_byte == b'-' {
                Err(Error::new("Negative reply"))
            } else {
                Err(Error::new("Unexpected reply"))
            }
        })
    }

    fn send_command(
        self,
        cmd: &str,
        args: Option<&str>,
    ) -> impl Future<Item = Self, Error = Error> {
        let mut hs = Some(self);
        let cmd = match args {
            Some(ref args) => format!("{} {}\r\n", cmd, args),
            None => format!("{}\r\n", cmd),
        };
        let mut idx = 0;

        future::poll_fn(move || {
            let bytes = cmd.as_bytes();

            loop {
                let hs = hs.as_mut().unwrap();

                if idx < bytes.len() {
                    let len = try_ready!(hs.socket.poll_write(&bytes[idx..]));
                    if len == 0 {
                        return Err(Error::new("Reached EOF"));
                    }

                    idx += len;
                } else {
                    break;
                }
            }

            Ok(Async::Ready(hs.take().unwrap()))
        })
        .and_then(|hs| hs.read_reply())
    }

    /// Send the `CONNECT <via_tag>` command to the server and wait for a positive reply.
    pub fn send_connect_command(self, via_tag: &str) -> impl Future<Item = Self, Error = Error> {
        self.send_command("CONNECT", Some(via_tag))
    }

    /// Send the `PASS <password>` command to the server and wait for a positive reply.
    pub fn send_pass_command(self, password: &str) -> impl Future<Item = Self, Error = Error> {
        self.send_command("PASS", Some(password))
    }

    /// Send the `CHANNEL <channel>` command to the server and wait for a positive reply.
    pub fn send_channel_command(self, channel: u8) -> impl Future<Item = Self, Error = Error> {
        self.send_command("CHANNEL", Some(&format!("{}", channel)))
    }

    /// Send the `DATA` command to the server and wait for a positive reply.
    pub fn send_data_command(self) -> impl Future<Item = TcpStream, Error = Error> {
        self.send_command("DATA", None).map(|hs| hs.into_inner())
    }

    /// Send the `QUIT` command to the server and wait for a positive reply.
    pub fn send_quit_command(self) -> impl Future<Item = (), Error = Error> {
        self.send_command("QUIT", None).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use std::net::Shutdown;

    use crate::{error::Result, tcp_server_handshake::TcpServerHandshake};
    use tokio::net::TcpListener;

    use super::*;

    fn wait_for_close(mut socket: TcpStream) -> impl Future<Item = (), Error = Error> {
        future::poll_fn(move || {
            let mut buf = [0; 256];
            let len = try_ready!(socket.poll_read(&mut buf));

            if len != 0 {
                Err(Error::new(format!("Read {} bytes...", len)))
            } else {
                Ok(Async::Ready(()))
            }
        })
    }

    #[test]
    fn test() -> Result<()> {
        let addr = "127.0.0.1:0".parse()?;
        let mut listener = TcpListener::bind(&addr)?;
        let addr = listener.local_addr()?;

        let handler = future::lazy(move || {
            let server = future::poll_fn(move || {
                let (socket, _addr) = try_ready!(listener.poll_accept());
                Ok(Async::Ready(socket))
            })
            .map_err(|err: std::io::Error| {
                panic!("{}", err);
            })
            .and_then(|socket| TcpServerHandshake::start(socket))
            .and_then(|hs| hs.receive_connect_command())
            .and_then(|(hs, via_tag)| {
                assert_eq!("via_tag", via_tag);

                hs.receive_pass_command()
            })
            .and_then(|(hs, password)| {
                assert_eq!("password", password);

                hs.receive_channel_command()
            })
            .and_then(|(hs, channel)| {
                assert_eq!(123, channel);

                hs.receive_data_command()
            })
            .and_then(|socket| {
                socket
                    .shutdown(Shutdown::Write)
                    .expect("Unable to shutdown server");
                wait_for_close(socket)
            })
            .map_err(|err| panic!("Server error: {}", err));

            let client = TcpStream::connect(&addr)
                .map_err(|err| Error::new(err))
                .and_then(|socket| TcpClientHandshake::start(socket))
                .and_then(|hs| hs.send_connect_command("via_tag"))
                .and_then(|hs| hs.send_pass_command("password"))
                .and_then(|hs| hs.send_channel_command(123))
                .and_then(|hs| hs.send_data_command())
                .and_then(|socket| {
                    socket
                        .shutdown(Shutdown::Write)
                        .expect("Unable to shutdown client");
                    wait_for_close(socket)
                })
                .map_err(|err| {
                    panic!("Client error: {}", err);
                });

            tokio::spawn(server);
            tokio::spawn(client);

            Ok(())
        });

        println!("Starting runtime...");

        tokio::run(handler);

        println!("Runtime ended");

        Ok(())
    }
}
