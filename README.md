# tokio-resol-vbus.rs

A library to wrap the `resol-vbus` crate to be used asynchronously
in conjunction with the `tokio` crate.

[![Crates.io][crates-badge]][crates-url]
[![Travis Build Status][travis-badge]][travis-url]

[crates-badge]: https://img.shields.io/crates/v/tokio-resol-vbus.svg
[crates-url]: https://crates.io/crates/tokio-resol-vbus
[travis-badge]: https://travis-ci.org/danielwippermann/tokio-resol-vbus.rs.svg?branch=master
[travis-url]: https://travis-ci.org/danielwippermann/tokio-resol-vbus.rs

[Documentation](https://docs.rs/tokio-resol-vbus/)
[Repository](https://github.com/danielwippermann/tokio-resol-vbus.rs)


## Examples

### Recorder of live VBus data into a persistent file format

```rust
use std::fs::File;

use resol_vbus::{DataSet, RecordingWriter};
use tokio::{net::TcpStream, prelude::*};

use tokio_resol_vbus::{Error, LiveDataStream, TcpClientHandshake};

fn main() {
    // Create an recording file and hand it to a `RecordingWriter`
    let file = File::create("test.vbus").expect("Unable to create output file");
    let mut rw = RecordingWriter::new(file);

    // Parse the address of the DL2 to connect to
    let addr = "192.168.13.45:7053"
        .parse()
        .expect("Unable to parse address");

    // Connect to the DL2
    let handler = TcpStream::connect(&addr)
        .map_err(Error::from)

        // Start the handshake
        .and_then(TcpClientHandshake::start)

        // Authenticate using a password
        .and_then(|hs| hs.send_pass_command("vbus"))

        // Switch to VBus data mode
        .and_then(|hs| hs.send_data_command())

        .and_then(|socket| {
            // Wrap the socket in a VBus `LiveDataStream`
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

    // Start the tokio runtime
    tokio::run(handler);
}
```
