use std::fs::File;

use resol_vbus::{DataSet, RecordingWriter};
use tokio::{net::TcpStream, prelude::*};

use tokio_resol_vbus::{Error, LiveDataStream, TcpClientHandshake};

fn main() {
    // Create an recording file and hand it to a `RecordingWriter`
    let file = File::create("test.vbus").expect("Unable to create output file");
    let mut rw = RecordingWriter::new(file);

    let addr = "192.168.13.45:7053"
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

            // Read VBus `Data` values from the `LiveDataStream`
            stream.for_each(move |data| {
                println!("{}", data.id_string());

                // Add `Data` value into `DataSet` to be stored
                let mut data_set = DataSet::new();
                data_set.timestamp = data.as_ref().timestamp;
                data_set.add_data(data);

                // Write the `DataSet` into the `RecordingWriter` for permanent storage
                rw.write_data_set(&data_set)
                    .expect("Unable to write data set");

                Ok(())
            })
        })
        .map_err(|err| {
            eprintln!("{}", err);
        });

    tokio::run(handler);
}
