use std::{fs::File, time::Duration};

use resol_vbus::{DataSet, RecordingWriter};
use tokio::prelude::*;
use tokio_serial::{DataBits, FlowControl, Parity, Serial, SerialPortSettings, StopBits};

use tokio_resol_vbus::LiveDataStream;

fn main() {
    // Create an recording file and hand it to a `RecordingWriter`
    let file = File::create("test.vbus").expect("Unable to create output file");
    let mut rw = RecordingWriter::new(file);

    let path = "/dev/tty.usbmodem";

    let settings = SerialPortSettings {
        baud_rate: 9600,
        data_bits: DataBits::Eight,
        flow_control: FlowControl::None,
        parity: Parity::None,
        stop_bits: StopBits::One,
        timeout: Duration::from_millis(500),
    };

    let serial = Serial::from_path(&path, &settings).expect("Unable to open serial port");
    let (reader, writer) = serial.split();
    let stream = LiveDataStream::new(reader, writer, 0, 0x0020);

    // Read VBus `Data` values from the `LiveDataStream`
    let handler = stream
        .for_each(move |data| {
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
        .map_err(|err| {
            eprintln!("{}", err);
        });

    tokio::run(handler);
}
