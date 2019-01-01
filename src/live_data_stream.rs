#![allow(clippy::if_same_then_else)]
#![allow(clippy::needless_bool)]

use std::{
    fmt,
    time::{Duration, Instant},
};

use futures::{try_ready, StartSend};
use resol_vbus::{
    chrono::Utc,
    live_data_encoder::{bytes_from_data, length_from_data},
    Data, Datagram, Header, LiveDataBuffer,
};
use tokio::{prelude::*, timer::Delay};

use crate::error::Error;

/// A `Stream`/`Sink` wrapper for RESOL VBus `Data` items.
#[derive(Debug)]
pub struct LiveDataStream<R: AsyncRead, W: AsyncWrite> {
    reader: R,
    writer: W,
    channel: u8,
    self_address: u16,
    buf: LiveDataBuffer,
}

impl<R: AsyncRead, W: AsyncWrite> LiveDataStream<R, W> {
    /// Create a new `LiveDataStream`.
    pub fn new(reader: R, writer: W, channel: u8, self_address: u16) -> LiveDataStream<R, W> {
        LiveDataStream {
            reader,
            writer,
            channel,
            self_address,
            buf: LiveDataBuffer::new(channel),
        }
    }

    /// Consume `self` and return the underlying I/O pair.
    pub fn into_inner(self) -> (R, W) {
        let LiveDataStream { reader, writer, .. } = self;
        (reader, writer)
    }

    fn create_datagram(
        &self,
        destination_address: u16,
        command: u16,
        param16: i16,
        param32: i32,
    ) -> Datagram {
        Datagram {
            header: Header {
                timestamp: Utc::now(),
                channel: self.channel,
                destination_address,
                source_address: self.self_address,
                protocol_version: 0x20,
            },
            command,
            param16,
            param32,
        }
    }

    /// Wait for any VBus data.
    pub fn wait_for_data(
        self,
        timeout: u64,
    ) -> impl Future<Item = (Self, Option<Data>), Error = Error> {
        ActionFuture::new(self, None, 1, timeout, 0, Box::new(|_| true))
    }

    /// Wait for a datagram that offers VBus control.
    pub fn wait_for_free_bus(self) -> impl Future<Item = (Self, Option<Data>), Error = Error> {
        ActionFuture::new(
            self,
            None,
            1,
            20000,
            0,
            Box::new(|data| {
                if let Data::Datagram(ref dgram) = *data {
                    if dgram.command != 0x0500 {
                        false
                    } else {
                        true
                    }
                } else {
                    false
                }
            }),
        )
    }

    /// Give back bus control to the regular VBus master.
    pub fn release_bus(
        self,
        address: u16,
    ) -> impl Future<Item = (Self, Option<Data>), Error = Error> {
        let tx_dgram = self.create_datagram(address, 0x0600, 0, 0);

        let tx_data = Some(Data::Datagram(tx_dgram));

        ActionFuture::new(
            self,
            tx_data,
            2,
            2500,
            2500,
            Box::new(|data| data.is_packet()),
        )
    }

    /// Get a value by its index.
    pub fn get_value_by_index(
        self,
        address: u16,
        index: i16,
        subindex: u8,
    ) -> impl Future<Item = (Self, Option<Data>), Error = Error> {
        let tx_dgram = self.create_datagram(address, 0x0300 | u16::from(subindex), index, 0);

        let tx_data = Some(Data::Datagram(tx_dgram.clone()));

        ActionFuture::new(
            self,
            tx_data,
            3,
            500,
            500,
            Box::new(move |data| {
                if let Data::Datagram(ref dgram) = *data {
                    if dgram.header.source_address != tx_dgram.header.destination_address {
                        false
                    } else if dgram.header.destination_address != tx_dgram.header.source_address {
                        false
                    } else if dgram.command != (0x0100 | u16::from(subindex)) {
                        false
                    } else if dgram.param16 != tx_dgram.param16 {
                        false
                    } else {
                        true
                    }
                } else {
                    false
                }
            }),
        )
    }

    /// Set a value by its index.
    pub fn set_value_by_index(
        self,
        address: u16,
        index: i16,
        subindex: u8,
        value: i32,
    ) -> impl Future<Item = (Self, Option<Data>), Error = Error> {
        let tx_dgram = self.create_datagram(address, 0x0200 | u16::from(subindex), index, value);

        let tx_data = Some(Data::Datagram(tx_dgram.clone()));

        ActionFuture::new(
            self,
            tx_data,
            3,
            500,
            500,
            Box::new(move |data| {
                if let Data::Datagram(ref dgram) = *data {
                    if dgram.header.source_address != tx_dgram.header.destination_address {
                        false
                    } else if dgram.header.destination_address != tx_dgram.header.source_address {
                        false
                    } else if dgram.command != (0x0100 | u16::from(subindex)) {
                        false
                    } else if dgram.param16 != tx_dgram.param16 {
                        false
                    } else {
                        true
                    }
                } else {
                    false
                }
            }),
        )
    }

    /// Get a value's ID hash by its index.
    pub fn get_value_id_hash_by_index(
        self,
        address: u16,
        index: i16,
    ) -> impl Future<Item = (Self, Option<Data>), Error = Error> {
        let tx_dgram = self.create_datagram(address, 0x1000, index, 0);

        let tx_data = Some(Data::Datagram(tx_dgram.clone()));

        ActionFuture::new(
            self,
            tx_data,
            3,
            500,
            500,
            Box::new(move |data| {
                if let Data::Datagram(ref dgram) = *data {
                    if dgram.header.source_address != tx_dgram.header.destination_address {
                        false
                    } else if dgram.header.destination_address != tx_dgram.header.source_address {
                        false
                    } else if dgram.command != 0x0100 {
                        false
                    } else if dgram.param16 != tx_dgram.param16 {
                        false
                    } else {
                        true
                    }
                } else {
                    false
                }
            }),
        )
    }

    /// Get a value's index by its ID hash.
    pub fn get_value_index_by_id_hash(
        self,
        address: u16,
        id_hash: i32,
    ) -> impl Future<Item = (Self, Option<Data>), Error = Error> {
        let tx_dgram = self.create_datagram(address, 0x1100, 0, id_hash);

        let tx_data = Some(Data::Datagram(tx_dgram.clone()));

        ActionFuture::new(
            self,
            tx_data,
            3,
            500,
            500,
            Box::new(move |data| {
                if let Data::Datagram(ref dgram) = *data {
                    if dgram.header.source_address != tx_dgram.header.destination_address {
                        false
                    } else if dgram.header.destination_address != tx_dgram.header.source_address {
                        false
                    } else if dgram.command != 0x0100 {
                        false
                    } else if dgram.param32 != tx_dgram.param32 {
                        false
                    } else {
                        true
                    }
                } else {
                    false
                }
            }),
        )
    }

    /// Get the capabilities (part 1) from a VBus device.
    pub fn get_caps1(
        self,
        address: u16,
    ) -> impl Future<Item = (Self, Option<Data>), Error = Error> {
        let tx_dgram = self.create_datagram(address, 0x1300, 0, 0);

        let tx_data = Some(Data::Datagram(tx_dgram.clone()));

        ActionFuture::new(
            self,
            tx_data,
            3,
            500,
            500,
            Box::new(move |data| {
                if let Data::Datagram(ref dgram) = *data {
                    if dgram.header.source_address != tx_dgram.header.destination_address {
                        false
                    } else if dgram.header.destination_address != tx_dgram.header.source_address {
                        false
                    } else if dgram.command != 0x1301 {
                        false
                    } else {
                        true
                    }
                } else {
                    false
                }
            }),
        )
    }

    /// Begin a bulk value transaction.
    pub fn begin_bulk_value_transaction(
        self,
        address: u16,
        tx_timeout: i32,
    ) -> impl Future<Item = (Self, Option<Data>), Error = Error> {
        let tx_dgram = self.create_datagram(address, 0x1400, 0, tx_timeout);

        let tx_data = Some(Data::Datagram(tx_dgram.clone()));

        ActionFuture::new(
            self,
            tx_data,
            3,
            500,
            500,
            Box::new(move |data| {
                if let Data::Datagram(ref dgram) = *data {
                    if dgram.header.source_address != tx_dgram.header.destination_address {
                        false
                    } else if dgram.header.destination_address != tx_dgram.header.source_address {
                        false
                    } else if dgram.command != 0x1401 {
                        false
                    } else {
                        true
                    }
                } else {
                    false
                }
            }),
        )
    }

    /// Commit a bulk value transaction.
    pub fn commit_bulk_value_transaction(
        self,
        address: u16,
    ) -> impl Future<Item = (Self, Option<Data>), Error = Error> {
        let tx_dgram = self.create_datagram(address, 0x1402, 0, 0);

        let tx_data = Some(Data::Datagram(tx_dgram.clone()));

        ActionFuture::new(
            self,
            tx_data,
            3,
            500,
            500,
            Box::new(move |data| {
                if let Data::Datagram(ref dgram) = *data {
                    if dgram.header.source_address != tx_dgram.header.destination_address {
                        false
                    } else if dgram.header.destination_address != tx_dgram.header.source_address {
                        false
                    } else if dgram.command != 0x1403 {
                        false
                    } else {
                        true
                    }
                } else {
                    false
                }
            }),
        )
    }

    /// Rollback a bulk value transaction.
    pub fn rollback_bulk_value_transaction(
        self,
        address: u16,
    ) -> impl Future<Item = (Self, Option<Data>), Error = Error> {
        let tx_dgram = self.create_datagram(address, 0x1404, 0, 0);

        let tx_data = Some(Data::Datagram(tx_dgram.clone()));

        ActionFuture::new(
            self,
            tx_data,
            3,
            500,
            500,
            Box::new(move |data| {
                if let Data::Datagram(ref dgram) = *data {
                    if dgram.header.source_address != tx_dgram.header.destination_address {
                        false
                    } else if dgram.header.destination_address != tx_dgram.header.source_address {
                        false
                    } else if dgram.command != 0x1405 {
                        false
                    } else {
                        true
                    }
                } else {
                    false
                }
            }),
        )
    }

    /// Set a value by its index while inside a bulk value transaction.
    pub fn set_bulk_value_by_index(
        self,
        address: u16,
        index: i16,
        subindex: u8,
        value: i32,
    ) -> impl Future<Item = (Self, Option<Data>), Error = Error> {
        let tx_dgram = self.create_datagram(address, 0x1500 | u16::from(subindex), index, value);

        let tx_data = Some(Data::Datagram(tx_dgram.clone()));

        ActionFuture::new(
            self,
            tx_data,
            3,
            500,
            500,
            Box::new(move |data| {
                if let Data::Datagram(ref dgram) = *data {
                    if dgram.header.source_address != tx_dgram.header.destination_address {
                        false
                    } else if dgram.header.destination_address != tx_dgram.header.source_address {
                        false
                    } else if dgram.command != (0x1600 | u16::from(subindex)) {
                        false
                    } else if dgram.param16 != tx_dgram.param16 {
                        false
                    } else {
                        true
                    }
                } else {
                    false
                }
            }),
        )
    }
}

impl<R: AsyncRead, W: AsyncWrite> Stream for LiveDataStream<R, W> {
    type Item = Data;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Data>, Error> {
        // println!("LiveDataStream::poll called");
        loop {
            // println!("  loop");
            if self.buf.peek_length().is_some() {
                break;
            }

            // println!("  reader.poll()");
            let mut buf = [0u8; 256];
            let len = try_ready!(self.reader.poll_read(&mut buf));
            // println!("  reader.poll() returned {} bytes @ {:?}", len, Utc::now());
            if len == 0 {
                return Ok(Async::Ready(None));
            }

            self.buf.extend_from_slice(&buf[0..len]);
        }

        let data = self.buf.read_data();
        // println!("  data = {:?}", data);
        Ok(Async::Ready(data))
    }
}

impl<R: AsyncRead, W: AsyncWrite> Sink for LiveDataStream<R, W> {
    type SinkItem = Data;
    type SinkError = Error;

    fn start_send(&mut self, data: Data) -> StartSend<Data, Error> {
        let len = length_from_data(&data);
        let mut buf = Vec::with_capacity(len);
        buf.resize(len, 0);
        bytes_from_data(&data, &mut buf);

        match self.writer.poll_write(&buf) {
            Ok(Async::Ready(written_len)) => {
                if written_len == len {
                    Ok(AsyncSink::Ready)
                } else {
                    Err(Error::new("Unable to write all bytes at once"))
                }
            }
            Ok(Async::NotReady) => Ok(AsyncSink::NotReady(data)),
            Err(err) => Err(Error::new(err)),
        }
    }

    fn poll_complete(&mut self) -> Poll<(), Error> {
        self.writer.poll_flush().map_err(Error::new)
    }
}

#[cfg(test)]
impl<R: AsyncRead, W: AsyncWrite> LiveDataStream<R, W> {
    pub fn writer_ref(&self) -> &W {
        &self.writer
    }
}

#[derive(Debug)]
enum ActionFuturePhase {
    Sending,
    Receiving,
    Done,
}

struct ActionFuture<R: AsyncRead, W: AsyncWrite> {
    stream: Option<LiveDataStream<R, W>>,
    tx_data: Option<Vec<u8>>,
    max_tries: usize,
    timeout: Duration,
    timeout_increment: Duration,
    filter: Box<dyn Fn(&Data) -> bool + Send + 'static>,
    phase: ActionFuturePhase,
    current_try: usize,
    delay: Option<Delay>,
}

impl<R: AsyncRead, W: AsyncWrite> ActionFuture<R, W> {
    fn new(
        stream: LiveDataStream<R, W>,
        tx_data: Option<Data>,
        max_tries: usize,
        initial_timeout_ms: u64,
        timeout_increment_ms: u64,
        filter: Box<dyn Fn(&Data) -> bool + Send + 'static>,
    ) -> ActionFuture<R, W> {
        let tx_data = tx_data.map(|ref data| {
            let len = length_from_data(data);
            let mut buf = Vec::with_capacity(len);
            buf.resize(len, 0);
            bytes_from_data(data, &mut buf);
            buf
        });

        ActionFuture {
            stream: Some(stream),
            tx_data,
            max_tries,
            timeout: Duration::from_millis(initial_timeout_ms),
            timeout_increment: Duration::from_millis(timeout_increment_ms),
            filter,
            phase: ActionFuturePhase::Sending,
            current_try: 0,
            delay: None,
        }
    }
}

impl<R: AsyncRead + fmt::Debug, W: AsyncWrite + fmt::Debug> fmt::Debug for ActionFuture<R, W> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ActionFuture")
            .field("stream", &self.stream)
            .field("tx_data", &self.tx_data)
            .field("max_tries", &self.max_tries)
            .field("timeout", &self.timeout)
            .field("timeout_increment", &self.timeout_increment)
            .field("filter", &"...")
            .field("phase", &self.phase)
            .field("current_try", &self.current_try)
            .field("delay", &self.delay)
            .finish()
    }
}

impl<R: AsyncRead, W: AsyncWrite> Future for ActionFuture<R, W> {
    type Item = (LiveDataStream<R, W>, Option<Data>);
    type Error = Error;

    fn poll(&mut self) -> Poll<(LiveDataStream<R, W>, Option<Data>), Error> {
        // println!("ActionFuture::poll called()");
        loop {
            // println!("  {:?}", self.phase);
            match self.phase {
                ActionFuturePhase::Sending => {
                    if let Some(ref data) = self.tx_data {
                        let len = try_ready!(self.stream.as_mut().unwrap().writer.poll_write(data));
                        if len != data.len() {
                            return Err(Error::new("Unable to write all bytes at once"));
                        }
                    }
                    self.phase = ActionFuturePhase::Receiving;
                    self.delay = Some(Delay::new(Instant::now() + self.timeout));
                }
                ActionFuturePhase::Receiving => {
                    // println!("  stream.poll()");
                    match self.stream.as_mut().unwrap().poll()? {
                        Async::Ready(data) => {
                            if let Some(data) = data {
                                // println!("    {:?}", data);
                                if (self.filter)(&data) {
                                    // println!("      ready!");
                                    self.delay = None;
                                    self.phase = ActionFuturePhase::Done;
                                    return Ok(Async::Ready((
                                        self.stream.take().unwrap(),
                                        Some(data),
                                    )));
                                }
                            } else {
                                return Err(Error::new("Reached EOF"));
                            }
                        }
                        Async::NotReady => {
                            // println!("   delay.poll()");
                            try_ready!(self.delay.as_mut().unwrap().poll());

                            self.timeout += self.timeout_increment;
                            self.current_try += 1;
                            self.delay = None;

                            // println!("   try: {}/{}", self.current_try, self.max_tries);
                            if self.current_try < self.max_tries {
                                self.phase = ActionFuturePhase::Sending;
                            } else {
                                self.phase = ActionFuturePhase::Done;
                                return Ok(Async::Ready((self.stream.take().unwrap(), None)));
                            }
                        }
                    }
                }
                ActionFuturePhase::Done => {
                    unreachable!();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use resol_vbus::Packet;

    use super::*;

    fn extend_from_data(buf: &mut Vec<u8>, data: &Data) {
        let len = length_from_data(data);
        let idx = buf.len();
        buf.resize(idx + len, 0);
        bytes_from_data(data, &mut buf[idx..]);
    }

    fn extend_with_empty_packet(
        buf: &mut Vec<u8>,
        destination_address: u16,
        source_address: u16,
        command: u16,
    ) {
        let data = Data::Packet(Packet {
            header: Header {
                timestamp: Utc::now(),
                channel: 0,
                destination_address,
                source_address,
                protocol_version: 0x20,
            },
            command,
            frame_count: 0,
            frame_data: [0; 508],
        });
        extend_from_data(buf, &data);
    }

    fn extend_from_datagram(
        buf: &mut Vec<u8>,
        destination_address: u16,
        source_address: u16,
        command: u16,
        param16: i16,
        param32: i32,
    ) {
        let data = Data::Datagram(Datagram {
            header: Header {
                timestamp: Utc::now(),
                channel: 0,
                destination_address,
                source_address,
                protocol_version: 0x20,
            },
            command,
            param16,
            param32,
        });
        extend_from_data(buf, &data);
    }

    fn simulate_run<T, E: fmt::Display + fmt::Debug, F: Future<Item = T, Error = E>>(
        mut f: F,
    ) -> T {
        match f.poll().expect("Unable to poll future") {
            Async::Ready(value) => value,
            Async::NotReady => panic!("Future returned NotReady"),
        }
    }

    trait ToBytes {
        fn to_bytes(&self) -> Vec<u8>;
    }

    fn hex_encode<T: ToBytes>(value: &T) -> String {
        let buf = value.to_bytes();
        buf.iter()
            .map(|b| format!("{:02x}", b))
            .collect::<Vec<String>>()
            .concat()
    }

    impl ToBytes for Cursor<Vec<u8>> {
        fn to_bytes(&self) -> Vec<u8> {
            self.get_ref().clone()
        }
    }

    impl ToBytes for Data {
        fn to_bytes(&self) -> Vec<u8> {
            let len = length_from_data(self);
            let mut buf = Vec::new();
            buf.resize(len, 0);
            bytes_from_data(self, &mut buf);
            buf
        }
    }

    #[test]
    fn test_wait_for_free_bus() {
        let mut rx_buf = Vec::new();
        let tx_buf = Cursor::new(Vec::new());

        extend_with_empty_packet(&mut rx_buf, 0x0010, 0x7E11, 0x0100);
        extend_from_datagram(&mut rx_buf, 0x0000, 0x7E11, 0x0500, 0, 0);

        let lds = LiveDataStream::new(&rx_buf[..], tx_buf, 0, 0x0020);

        let (lds, data) = simulate_run(lds.wait_for_free_bus());

        assert_eq!("", hex_encode(lds.writer_ref()));
        assert_eq!(
            "aa0000117e200005000000000000004b",
            hex_encode(&data.unwrap())
        );
    }

    #[test]
    fn test_release_bus() {
        let mut rx_buf = Vec::new();
        let tx_buf = Cursor::new(Vec::new());

        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x0100, 0, 0);
        extend_with_empty_packet(&mut rx_buf, 0x0010, 0x7E11, 0x0100);

        let lds = LiveDataStream::new(&rx_buf[..], tx_buf, 0, 0x0020);

        let (lds, data) = simulate_run(lds.release_bus(0x7E11));

        assert_eq!(
            "aa117e2000200006000000000000002a",
            hex_encode(lds.writer_ref())
        );
        assert_eq!("aa1000117e100001004f", hex_encode(&data.unwrap()));
    }

    #[test]
    fn test_get_value_by_index() {
        let mut rx_buf = Vec::new();
        let tx_buf = Cursor::new(Vec::new());

        extend_with_empty_packet(&mut rx_buf, 0x0010, 0x7E11, 0x0100);
        extend_from_datagram(&mut rx_buf, 0x0021, 0x7E11, 0x0156, 0x1234, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E10, 0x0156, 0x1234, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x0157, 0x1234, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x0157, 0x1235, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x0156, 0x1234, 0x789abcde);

        let lds = LiveDataStream::new(&rx_buf[..], tx_buf, 0, 0x0020);

        let (lds, data) = simulate_run(lds.get_value_by_index(0x7E11, 0x1234, 0x56));

        assert_eq!(
            "aa117e20002056033412000000000011",
            hex_encode(lds.writer_ref())
        );
        assert_eq!(
            "aa2000117e20560134125e3c1a781c4b",
            hex_encode(&data.unwrap())
        );
    }

    #[test]
    fn test_set_value_by_index() {
        let mut rx_buf = Vec::new();
        let tx_buf = Cursor::new(Vec::new());

        extend_with_empty_packet(&mut rx_buf, 0x0010, 0x7E11, 0x0100);
        extend_from_datagram(&mut rx_buf, 0x0021, 0x7E11, 0x0156, 0x1234, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E10, 0x0156, 0x1234, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x0157, 0x1234, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x0156, 0x1235, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x0156, 0x1234, 0x789abcde);

        let lds = LiveDataStream::new(&rx_buf[..], tx_buf, 0, 0x0020);

        let (lds, data) = simulate_run(lds.set_value_by_index(0x7E11, 0x1234, 0x56, 0x789abcde));

        assert_eq!(
            "aa117e200020560234125e3c1a781c4a",
            hex_encode(lds.writer_ref())
        );
        assert_eq!(
            "aa2000117e20560134125e3c1a781c4b",
            hex_encode(&data.unwrap())
        );
    }

    #[test]
    fn test_get_value_id_hash_by_index() {
        let mut rx_buf = Vec::new();
        let tx_buf = Cursor::new(Vec::new());

        extend_with_empty_packet(&mut rx_buf, 0x0010, 0x7E11, 0x0100);
        extend_from_datagram(&mut rx_buf, 0x0021, 0x7E11, 0x0100, 0x1234, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E10, 0x0100, 0x1234, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x0101, 0x1234, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x0100, 0x1235, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x0100, 0x1234, 0x789abcde);

        let lds = LiveDataStream::new(&rx_buf[..], tx_buf, 0, 0x0020);

        let (lds, data) = simulate_run(lds.get_value_id_hash_by_index(0x7E11, 0x1234));

        assert_eq!(
            "aa117e2000200010341200000000005a",
            hex_encode(lds.writer_ref())
        );
        assert_eq!(
            "aa2000117e20000134125e3c1a781c21",
            hex_encode(&data.unwrap())
        );
    }

    #[test]
    fn test_get_value_index_by_id_hash() {
        let mut rx_buf = Vec::new();
        let tx_buf = Cursor::new(Vec::new());

        extend_with_empty_packet(&mut rx_buf, 0x0010, 0x7E11, 0x0100);
        extend_from_datagram(&mut rx_buf, 0x0021, 0x7E11, 0x0100, 0x1234, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E10, 0x0100, 0x1234, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x0101, 0x1234, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x0100, 0x1234, 0x789abcdf);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x0100, 0x1234, 0x789abcde);

        let lds = LiveDataStream::new(&rx_buf[..], tx_buf, 0, 0x0020);

        let (lds, data) = simulate_run(lds.get_value_index_by_id_hash(0x7E11, 0x789abcde));

        assert_eq!(
            "aa117e200020001100005e3c1a781c57",
            hex_encode(lds.writer_ref())
        );
        assert_eq!(
            "aa2000117e20000134125e3c1a781c21",
            hex_encode(&data.unwrap())
        );
    }

    #[test]
    fn test_get_caps1() {
        let mut rx_buf = Vec::new();
        let tx_buf = Cursor::new(Vec::new());

        extend_with_empty_packet(&mut rx_buf, 0x0010, 0x7E11, 0x0100);
        extend_from_datagram(&mut rx_buf, 0x0021, 0x7E11, 0x1301, 0, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E10, 0x1301, 0, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x1300, 0, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x1301, 0, 0x789abcde);

        let lds = LiveDataStream::new(&rx_buf[..], tx_buf, 0, 0x0020);

        let (lds, data) = simulate_run(lds.get_caps1(0x7E11));

        assert_eq!(
            "aa117e2000200013000000000000001d",
            hex_encode(lds.writer_ref())
        );
        assert_eq!(
            "aa2000117e20011300005e3c1a781c54",
            hex_encode(&data.unwrap())
        );
    }

    #[test]
    fn test_begin_bulk_value_transaction() {
        let mut rx_buf = Vec::new();
        let tx_buf = Cursor::new(Vec::new());

        extend_with_empty_packet(&mut rx_buf, 0x0010, 0x7E11, 0x0100);
        extend_from_datagram(&mut rx_buf, 0x0021, 0x7E11, 0x1401, 0, 0);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E10, 0x1401, 0, 0);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x1400, 0, 0);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x1401, 0, 0);

        let lds = LiveDataStream::new(&rx_buf[..], tx_buf, 0, 0x0020);

        let (lds, data) = simulate_run(lds.begin_bulk_value_transaction(0x7E11, 0x789abcde));

        assert_eq!(
            "aa117e200020001400005e3c1a781c54",
            hex_encode(lds.writer_ref())
        );
        assert_eq!(
            "aa2000117e200114000000000000001b",
            hex_encode(&data.unwrap())
        );
    }

    #[test]
    fn test_commit_value_transaction() {
        let mut rx_buf = Vec::new();
        let tx_buf = Cursor::new(Vec::new());

        extend_with_empty_packet(&mut rx_buf, 0x0010, 0x7E11, 0x0100);
        extend_from_datagram(&mut rx_buf, 0x0021, 0x7E11, 0x1403, 0, 0);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E10, 0x1403, 0, 0);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x1402, 0, 0);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x1403, 0, 0);

        let lds = LiveDataStream::new(&rx_buf[..], tx_buf, 0, 0x0020);

        let (lds, data) = simulate_run(lds.commit_bulk_value_transaction(0x7E11));

        assert_eq!(
            "aa117e2000200214000000000000001a",
            hex_encode(lds.writer_ref())
        );
        assert_eq!(
            "aa2000117e2003140000000000000019",
            hex_encode(&data.unwrap())
        );
    }

    #[test]
    fn test_rollback_value_transaction() {
        let mut rx_buf = Vec::new();
        let tx_buf = Cursor::new(Vec::new());

        extend_with_empty_packet(&mut rx_buf, 0x0010, 0x7E11, 0x0100);
        extend_from_datagram(&mut rx_buf, 0x0021, 0x7E11, 0x1405, 0, 0);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E10, 0x1405, 0, 0);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x1404, 0, 0);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x1405, 0, 0);

        let lds = LiveDataStream::new(&rx_buf[..], tx_buf, 0, 0x0020);

        let (lds, data) = simulate_run(lds.rollback_bulk_value_transaction(0x7E11));

        assert_eq!(
            "aa117e20002004140000000000000018",
            hex_encode(lds.writer_ref())
        );
        assert_eq!(
            "aa2000117e2005140000000000000017",
            hex_encode(&data.unwrap())
        );
    }

    #[test]
    fn test_set_bulk_value_by_index() {
        let mut rx_buf = Vec::new();
        let tx_buf = Cursor::new(Vec::new());

        extend_with_empty_packet(&mut rx_buf, 0x0010, 0x7E11, 0x0100);
        extend_from_datagram(&mut rx_buf, 0x0021, 0x7E11, 0x1656, 0x1234, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E10, 0x1656, 0x1234, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x1657, 0x1234, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x1656, 0x1235, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x1656, 0x1234, 0x789abcde);

        let lds = LiveDataStream::new(&rx_buf[..], tx_buf, 0, 0x0020);

        let (lds, data) =
            simulate_run(lds.set_bulk_value_by_index(0x7E11, 0x1234, 0x56, 0x789abcde));

        assert_eq!(
            "aa117e200020561534125e3c1a781c37",
            hex_encode(lds.writer_ref())
        );
        assert_eq!(
            "aa2000117e20561634125e3c1a781c36",
            hex_encode(&data.unwrap())
        );
    }
}
