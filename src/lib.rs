// This is part of tokio-resol-vbus.rs.
// Copyright (c) 2018-2019, Daniel Wippermann.
// See README.md and LICENSE.txt for details.

//! A library to wrap the `resol-vbus` crate to be used asynchronously
//! in conjunction with the `tokio` crate.
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
