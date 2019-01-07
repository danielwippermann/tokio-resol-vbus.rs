// This is part of tokio-resol-vbus.rs.
// Copyright (c) 2018-2019, Daniel Wippermann.
// See README.md and LICENSE.txt for details.

//! # tokio-resol-vbus
//!
//! A library to wrap the `resol-vbus` crate to be used asynchronously
//! in conjunction with the `tokio` crate.
//!
//!
//! ## Features
//!
//! - Provides helpers for VBus-over-TCP handshake
//! - Allows to send and receive data to / from a controller asynchronously
//!
//!
//! ## Planned, but not yet implemented features
//!
//! - Discovers LAN-enabled RESOL devices on the local network
//!
//!
//! ## Supported Devices & Services
//!
//! * [All current RESOL controllers with VBus](http://www.resol.de/index/produkte/sprache/en)
//! * [RESOL DL2 Datalogger](http://www.resol.de/index/produktdetail/kategorie/2/sprache/en/id/12)
//! * [RESOL DL3 Datalogger](http://www.resol.de/index/produktdetail/kategorie/2/sprache/en/id/86)
//! * [RESOL VBus/LAN interface adapter](http://www.resol.de/index/produktdetail/kategorie/2/id/76/sprache/en)
//! * [RESOL VBus/USB interface adapter](http://www.resol.de/index/produktdetail/kategorie/2/id/13/sprache/en)
//! * [RESOL VBus.net](http://www.vbus.net/)
//!
//!
//! ## Technical Information & Specifications
//!
//! * [RESOL VBus Google Group](https://groups.google.com/forum/#!forum/resol-vbus)
//! * [RESOL VBus Protocol Specification](http://danielwippermann.github.io/resol-vbus/vbus-specification.html)
//! * [RESOL VBus Packet List](http://danielwippermann.github.io/resol-vbus/vbus-packets.html)
//! * [RESOL VBus Recording File Format](http://danielwippermann.github.io/resol-vbus/vbus-recording-file-format.html)
//! * [RESOL VBus Specification File Format v1](http://danielwippermann.github.io/resol-vbus/vbus-specification-file-format-v1.html)
//! * [RESOL VBus over TCP Specification](http://danielwippermann.github.io/resol-vbus/vbus-over-tcp.html)
//! * [RESOL DL2 (v1) Data Download API](https://drive.google.com/file/d/0B4wMTuLGRPi2YmM5ZTJiNDQtNjkyMi00ZWYzLTgzYzgtYTdiMjBlZmI5ODgx/edit?usp=sharing)
//! * [RESOL DL2 (v2) & DL3 Data Download API](http://danielwippermann.github.io/resol-vbus/dlx-data-download-api.html)
//!
//!
//! ## Examples
//!
//! ### Recorder of live VBus data into a persistent file format
//!
//! ```rust,no_run
//! use std::fs::File;
//!
//! use resol_vbus::{DataSet, RecordingWriter};
//! use tokio::{net::TcpStream, prelude::*};
//!
//! use tokio_resol_vbus::{Error, LiveDataStream, TcpClientHandshake};
//!
//! fn main() {
//!     // Create an recording file and hand it to a `RecordingWriter`
//!     let file = File::create("test.vbus").expect("Unable to create output file");
//!     let mut rw = RecordingWriter::new(file);
//!
//!     // Parse the address of the DL2 to connect to
//!     let addr = "192.168.13.45:7053"
//!         .parse()
//!         .expect("Unable to parse address");
//!
//!     // Connect to the DL2
//!     let handler = TcpStream::connect(&addr)
//!         .map_err(Error::from)
//!
//!         // Start the handshake
//!         .and_then(TcpClientHandshake::start)
//!
//!         // Authenticate using a password
//!         .and_then(|hs| hs.send_pass_command("vbus"))
//!
//!         // Switch to VBus data mode
//!         .and_then(|hs| hs.send_data_command())
//!
//!         .and_then(|socket| {
//!             // Wrap the socket in a VBus `LiveDataStream`
//!             let (reader, writer) = socket.split();
//!             let stream = LiveDataStream::new(reader, writer, 0, 0x0020);
//!
//!             // Read VBus `Data` values from the `LiveDataStream`
//!             stream.for_each(move |data| {
//!                 println!("{}", data.id_string());
//!
//!                 // Add `Data` value into `DataSet` to be stored
//!                 let mut data_set = DataSet::new();
//!                 data_set.timestamp = data.as_ref().timestamp;
//!                 data_set.add_data(data);
//!
//!                 // Write the `DataSet` into the `RecordingWriter` for permanent storage
//!                 rw.write_data_set(&data_set)
//!                     .expect("Unable to write data set");
//!
//!                 Ok(())
//!             })
//!         })
//!
//!         .map_err(|err| {
//!             eprintln!("{}", err);
//!         });
//!
//!     // Start the tokio runtime
//!     tokio::run(handler);
//! }
//! ```
#![warn(missing_docs)]
#![deny(bare_trait_objects)]
#![deny(missing_debug_implementations)]
#![deny(warnings)]

mod error;
mod live_data_stream;
mod tcp_client_handshake;
mod tcp_server_handshake;

pub use resol_vbus;
pub use resol_vbus::chrono;

pub use crate::{
    error::{Error, Result},
    live_data_stream::LiveDataStream,
    tcp_client_handshake::TcpClientHandshake,
    tcp_server_handshake::TcpServerHandshake,
};
