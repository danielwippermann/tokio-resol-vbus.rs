use tokio::{net::TcpStream, prelude::*};

use tokio_resol_vbus::{Error, LiveDataStream, TcpClientHandshake};

fn main() {
    let addr = "192.168.5.81:7053"
        .parse()
        .expect("Unable to parse address");

    let handler = TcpStream::connect(&addr)
        .map_err(Error::from)
        .and_then(TcpClientHandshake::start)
        .and_then(|hs| hs.send_pass_command("vbus"))
        .and_then(|hs| hs.send_data_command())
        .and_then(|socket| {
            let (reader, writer) = socket.split();
            let stream = LiveDataStream::new(reader, writer, 0, 0x0020);

            stream.wait_for_free_bus()
        })
        .and_then(|(stream, dgram)| match dgram {
            Some(ref dgram) => Ok((stream, dgram.header.source_address)),
            None => Err(Error::new("Unable to get VBus control")),
        })
        .and_then(|(stream, address)| {
            stream
                .get_value_by_index(address, 0, 0)
                .and_then(|(stream, dgram)| match dgram {
                    Some(ref dgram) => Ok((stream, dgram.param32)),
                    None => Err(Error::new("Unable to get changeset")),
                })
                .and_then(move |(stream, changeset)| {
                    println!(
                        "Controller 0x{:04X} has changeset 0x{:08X}",
                        address, changeset
                    );

                    stream.release_bus(address)
                })
        })
        .and_then(|_| {
            println!("Done!");

            Ok(())
        })
        .map_err(|err| {
            eprintln!("{}", err);
        });

    tokio::run(handler);
}
