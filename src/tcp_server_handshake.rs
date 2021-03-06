use std::result::Result as StdResult;

use futures::try_ready;
use resol_vbus::BlobBuffer;
use tokio::net::TcpStream;
use tokio::prelude::*;

use crate::error::{Error, Result};

type ReceiveCommandValidatorResult<T> = (&'static str, Option<Result<T>>);
type ReceiveCommandValidatorFutureBox<T> =
    Box<dyn Future<Item = ReceiveCommandValidatorResult<T>, Error = Error> + Send>;

type ReceiveCommandSendReplyResult<S, T> = (S, Option<Result<T>>);
type ReceiveCommandSendReplyFutureBox<S, T> =
    Box<dyn Future<Item = ReceiveCommandSendReplyResult<S, T>, Error = Error> + Send>;

/// Handles the server-side of the [VBus-over-TCP][1] handshake.
///
/// [1]: http://danielwippermann.github.io/resol-vbus/vbus-over-tcp.html
///
/// # Examples
///
/// This example simulates a RESOL DL2 by accepting TCP connections on port
/// 7053, requiring the client to provide a password and then switch the
/// connection into raw VBus data mode.
///
/// ```no_run
/// use tokio::prelude::*;
/// use tokio::net::TcpListener;
/// use tokio_resol_vbus::TcpServerHandshake;
///
/// let addr = "127.0.0.1:7053".parse().expect("Unable to parse address");
/// let listener = TcpListener::bind(&addr).expect("Unable to bind listener");
///
/// let server = listener
///     .incoming()
///     .map_err(|err| eprintln!("{}", err))
///     .for_each(|socket| {
///         let conn = TcpServerHandshake::start(socket)
///             .and_then(|hs| hs.receive_pass_command_and_verify_password(|password| {
///                 if password == "vbus" {
///                     Ok(Some(password))
///                 } else {
///                     Ok(None)
///                 }
///             }))
///             .and_then(|(hs, _)| hs.receive_data_command())
///             .and_then(|socket| {
///                 // do something with the socket
///                 # Ok(())
///             })
///             .map_err(|err| eprintln!("Server error: {}", err));
///         tokio::spawn(conn)
///     });
///
/// tokio::run(server);
/// ```
#[derive(Debug)]
pub struct TcpServerHandshake {
    socket: TcpStream,
    buf: BlobBuffer,
}

impl TcpServerHandshake {
    /// Start the VBus-over-TCP handshake as the client side connecting to a server.
    pub fn start(socket: TcpStream) -> impl Future<Item = Self, Error = Error> {
        let hs = TcpServerHandshake {
            socket,
            buf: BlobBuffer::new(),
        };

        hs.send_reply("+HELLO\r\n")
    }

    /// Consume `self` and return the underlying `TcpStream`.
    pub fn into_inner(self) -> TcpStream {
        self.socket
    }

    /// Send a reply to the client.
    fn send_reply(self, reply: &'static str) -> impl Future<Item = Self, Error = Error> {
        let mut hs = Some(self);
        let bytes = reply.as_bytes();
        let mut idx = 0;

        future::poll_fn(move || {
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
    }

    fn poll_receive_line(&mut self) -> Poll<String, Error> {
        loop {
            if let Some(idx) = self.buf.iter().position(|b| *b == 10) {
                let string = std::str::from_utf8(&self.buf)?.to_string();

                self.buf.consume(idx + 1);

                return Ok(Async::Ready(string));
            }

            let mut tmp_buf = [0u8; 256];
            let len = try_ready!(self.socket.poll_read(&mut tmp_buf));
            if len == 0 {
                return Err(Error::new("Reached EOF"));
            }

            self.buf.extend_from_slice(&tmp_buf[0..len]);
        }
    }

    /// Receive a command and verify it and its provided arguments. The
    /// command reception is repeated as long as the verification fails.
    ///
    /// The preferred way to receive commands documented in the VBus-over-TCP
    /// specification is through the `receive_xxx_command` and
    /// `receive_xxx_command_and_verify_yyy` methods which use the
    /// `receive_command` method internally.
    ///
    /// This method takes a validator function that is called with the
    /// received command and its optional arguments. The validator
    /// returns a `Future` that can resolve into an
    /// `std::result::Result<T, &'static str>`. It can either be:
    /// - `Ok(value)` if the validation succeeded. The `value` is used
    ///   to resolve the `receive_command` `Future`.
    /// - `Err(reply)` if the validation failed. The `reply` is send
    ///   back to the client and the command reception is repeated.
    pub fn receive_command<V, R, T>(
        self,
        validator: V,
    ) -> impl Future<Item = (Self, T), Error = Error>
    where
        V: Fn(String, Option<String>) -> R,
        R: Future<Item = StdResult<T, &'static str>, Error = Error> + Send + 'static,
        T: Send + 'static,
    {
        let mut self_option = Some(self);
        let mut future0: Option<ReceiveCommandValidatorFutureBox<T>> = None;
        let mut future1: Option<ReceiveCommandSendReplyFutureBox<Self, T>> = None;
        let mut phase = 0;

        future::poll_fn(move || loop {
            match phase {
                // Phase 0:
                // - `self` is stored in `self_option`
                // - create `receive_xxx_command` future
                0 => {
                    let line = try_ready!(self_option.as_mut().unwrap().poll_receive_line());
                    let line = line.trim();
                    let (command, args) =
                        if let Some(idx) = line.chars().position(|c| c.is_whitespace()) {
                            let command = (&line[0..idx]).to_uppercase();
                            let args = (&line[idx..].trim()).to_string();

                            (command, Some(args))
                        } else {
                            (line.to_string(), None)
                        };

                    let future: ReceiveCommandValidatorFutureBox<T> = if command == "QUIT" {
                        let result = ("+OK\r\n", Some(Err(Error::new("Received QUIT command"))));
                        Box::new(future::ok(result))
                    } else {
                        let future = validator(command, args);
                        Box::new(future.and_then(|result| {
                            let result = match result {
                                Ok(value) => ("+OK\r\n", Some(Ok(value))),
                                Err(reply) => (reply, None),
                            };
                            Ok(result)
                        }))
                    };

                    future0 = Some(future);
                    phase = 1;
                }
                1 => {
                    let (reply, result) = try_ready!(future0.as_mut().unwrap().poll());

                    let hs = self_option.take().unwrap();
                    let future = hs.send_reply(reply).and_then(|hs| Ok((hs, result)));

                    future1 = Some(Box::new(future));
                    phase = 2;
                }
                2 => {
                    let (hs, result) = try_ready!(future1.as_mut().unwrap().poll());
                    future1 = None;

                    if let Some(result) = result {
                        phase = 3;
                        match result {
                            Ok(value) => break Ok(Async::Ready((hs, value))),
                            Err(err) => break Err(err),
                        }
                    } else {
                        phase = 0;
                    }
                }
                // Phase 3:
                // - this future is already resolved
                // - panic!
                3 => panic!("Called poll() on resolved future"),
                _ => unreachable!(),
            }
        })
    }

    /// Wait for a `CONNECT <via_tag>` command. The via tag argument is returned.
    pub fn receive_connect_command(self) -> impl Future<Item = (Self, String), Error = Error> {
        self.receive_connect_command_and_verify_via_tag(|via_tag| Ok(Some(via_tag)))
    }

    /// Wait for a `CONNECT <via_tag>` command.
    pub fn receive_connect_command_and_verify_via_tag<V, F, R>(
        self,
        validator: V,
    ) -> impl Future<Item = (Self, String), Error = Error>
    where
        V: Fn(String) -> F,
        F: IntoFuture<Item = Option<String>, Error = Error, Future = R> + Send,
        R: Future<Item = Option<String>, Error = Error> + Send + 'static,
    {
        self.receive_command(move |command, args| {
            let result = if command != "CONNECT" {
                Err("-ERROR Expected CONNECT command\r\n")
            } else if let Some(via_tag) = args {
                Ok(via_tag)
            } else {
                Err("-ERROR Expected argument\r\n")
            };

            let future: Box<
                dyn Future<Item = StdResult<String, &'static str>, Error = Error> + Send,
            > = match result {
                Ok(via_tag) => {
                    Box::new(validator(via_tag).into_future().map(|result| match result {
                        Some(via_tag) => Ok(via_tag),
                        None => Err("-ERROR Invalid via tag\r\n"),
                    }))
                }
                Err(reply) => Box::new(future::ok(Err(reply))),
            };

            future
        })
    }

    /// Wait for a `PASS <password>` command.
    pub fn receive_pass_command(self) -> impl Future<Item = (Self, String), Error = Error> {
        self.receive_pass_command_and_verify_password(|password| Ok(Some(password)))
    }

    /// Wait for a `PASS <password>` command and validate the provided password.
    pub fn receive_pass_command_and_verify_password<V, F, R>(
        self,
        validator: V,
    ) -> impl Future<Item = (Self, String), Error = Error>
    where
        V: Fn(String) -> F,
        F: IntoFuture<Item = Option<String>, Error = Error, Future = R> + Send,
        R: Future<Item = Option<String>, Error = Error> + Send + 'static,
    {
        self.receive_command(move |command, args| {
            let result = if command != "PASS" {
                Err("-ERROR Expected PASS command\r\n")
            } else if let Some(password) = args {
                Ok(password)
            } else {
                Err("-ERROR Expected argument\r\n")
            };

            let future: Box<
                dyn Future<Item = StdResult<String, &'static str>, Error = Error> + Send,
            > = match result {
                Ok(password) => Box::new(validator(password).into_future().map(
                    |result| match result {
                        Some(password) => Ok(password),
                        None => Err("-ERROR Invalid password\r\n"),
                    },
                )),
                Err(reply) => Box::new(future::ok(Err(reply))),
            };

            future
        })
    }

    /// Wait for a `CHANNEL <channel>` command.
    pub fn receive_channel_command(self) -> impl Future<Item = (Self, u8), Error = Error> {
        self.receive_channel_command_and_verify_channel(|channel| Ok(Some(channel)))
    }

    /// Wait for `CHANNEL <channel>` command and validate the provided channel
    pub fn receive_channel_command_and_verify_channel<V, F, R>(
        self,
        validator: V,
    ) -> impl Future<Item = (Self, u8), Error = Error>
    where
        V: Fn(u8) -> F,
        F: IntoFuture<Item = Option<u8>, Error = Error, Future = R> + Send + 'static,
        R: Future<Item = Option<u8>, Error = Error> + Send + 'static,
    {
        self.receive_command(move |command, args| {
            let result = if command != "CHANNEL" {
                Err("-ERROR Expected CHANNEL command\r\n")
            } else if let Some(args) = args {
                if let Ok(channel) = args.parse::<u8>() {
                    Ok(channel)
                } else {
                    Err("-ERROR Expected 8 bit number argument\r\n")
                }
            } else {
                Err("-ERROR Expected argument\r\n")
            };

            let future: Box<dyn Future<Item = StdResult<u8, &'static str>, Error = Error> + Send> =
                match result {
                    Ok(channel) => {
                        Box::new(validator(channel).into_future().map(|result| match result {
                            Some(channel) => Ok(channel),
                            None => Err("-ERROR Invalid channel\r\n"),
                        }))
                    }
                    Err(reply) => Box::new(future::ok(Err(reply))),
                };

            future
        })
    }

    /// Wait for a `DATA` command.
    pub fn receive_data_command(self) -> impl Future<Item = TcpStream, Error = Error> {
        self.receive_command(|command, args| {
            let result = if command != "DATA" {
                Err("-ERROR Expected DATA command\r\n")
            } else if args.is_some() {
                Err("-ERROR Did not expect arguments\r\n")
            } else {
                Ok(())
            };

            future::ok(result)
        })
        .map(|(hs, _)| hs.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use std::net::Shutdown;

    use tokio::net::TcpListener;

    use crate::{error::Result, tcp_client_handshake::TcpClientHandshake};

    use super::*;

    fn wait_for_close(mut socket: TcpStream) -> impl Future<Item = (), Error = Error> {
        future::poll_fn(move || {
            let mut buf = [0; 256];
            let len = try_ready!(socket.poll_read(&mut buf));

            if len != 0 {
                Err(Error::new(format!(
                    "Read {} bytes: {:?}",
                    len,
                    std::str::from_utf8(&buf[0..len])
                )))
            } else {
                Ok(Async::Ready(()))
            }
        })
    }

    #[test]
    fn test() -> Result<()> {
        let addr = "127.0.0.1:7053".parse()?;
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
            .and_then(|(hs, args)| {
                assert_eq!("via_tag", args);
                hs.receive_pass_command_and_verify_password(|password| {
                    if password == "password" {
                        Ok(Some(password))
                    } else {
                        Ok(None)
                    }
                })
            })
            .and_then(|(hs, args)| {
                assert_eq!("password", args);
                hs.receive_channel_command_and_verify_channel(|channel| {
                    if channel == 123 {
                        Ok(Some(channel))
                    } else {
                        Ok(None)
                    }
                })
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
            .map_err(|err| {
                panic!("Server error: {}", err);
            });

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
