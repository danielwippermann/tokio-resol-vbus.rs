use resol_vbus::{Data, Datagram, Header};
use tokio::net::TcpListener;
use tokio::prelude::*;

use tokio_resol_vbus::{LiveDataStream, Result, TcpServerHandshake};

fn main() -> Result<()> {
    let addr = "127.0.0.1:7053".parse().expect("Unable to parse address");
    let listener = TcpListener::bind(&addr).expect("Unable to bind listener");

    let server = listener
        .incoming()
        .map_err(|err| eprintln!("{}", err))
        .for_each(|socket| {
            let conn = TcpServerHandshake::start(socket)
                .and_then(|hs| {
                    hs.receive_pass_command_and_verify_password(|password| {
                        if password == "vbus" {
                            Ok(Some(password))
                        } else {
                            Ok(None)
                        }
                    })
                })
                .and_then(|(hs, _)| hs.receive_data_command())
                .and_then(|socket| {
                    let (reader, writer) = socket.split();
                    let stream = LiveDataStream::new(reader, writer, 0, 0x7E11);

                    future::loop_fn(stream, |stream| {
                        let tx_data = Data::Datagram(Datagram {
                            header: Header {
                                destination_address: 0x0000,
                                source_address: 0x7E11,
                                protocol_version: 0x20,
                                ..Header::default()
                            },
                            command: 0x0500,
                            param16: 0,
                            param32: 0,
                        });

                        stream
                            .transceive(tx_data, 1, 1000, 0, |data| {
                                data.as_ref().destination_address == 0x7E11
                            })
                            .and_then(|(stream, data)| {
                                println!("{:?}", data);

                                Ok(future::Loop::Continue(stream))
                            })
                    })
                })
                .map_err(|err| {
                    panic!("Server error: {}", err);
                });
            tokio::spawn(conn)
        });

    tokio::run(server);

    Ok(())
}
