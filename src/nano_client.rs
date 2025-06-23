//! Nano MQTT client - a small and opinionated MQTT client
//!
//! This client can be used to subscribe to a predefined set of topics, and publish
//! messages. It will handle connecting the transport, sending the initial subscribe
//! and sending pings as well as responding to publish messages etc.
//!
//! It uses externally owned buffers, so the user is in total control where and how buffers
//! are allocated.
//!
//! This client does not handle packet id allocation, publish/republish or other state management.
//! It is up to the user to ensure that packet ids don't collide when publishing data, as well
//! as finishing published data etc.
//!
//! **Note:** The initial subscribe message will always have packet ID 1.
//!
//! A basic example client is as follows:
//!
//! ```no_run
//! use std::time::Duration;
//! use tokio::io::{AsyncReadExt, AsyncWriteExt};
//! use tokio::net::TcpStream;
//! use umqtt::nano_client::{NanoClient, ClientNotification, Error, Platform};
//! use umqtt::packetview::connect::ConnectOptions;
//! use umqtt::packetview::QoS;
//! use umqtt::packetview::subscribe::{SubscribeFilter};
//! use umqtt::time::Instant;
//!
//! struct TokioPlatform {
//!     stream: TcpStream,
//!     epoch: tokio::time::Instant,
//! }
//!
//! impl Platform for TokioPlatform {
//!     type Error = std::io::Error;
//!
//!     async fn write_all(&mut self, buf: &[u8]) -> Result<(), Self::Error> {
//!         self.stream.write_all(buf).await
//!     }
//!
//!     async fn read_some(&mut self, buf: &mut [u8], timeout: Option<Duration>) -> Result<Option<usize>, Self::Error> {
//!         if let Some(timeout) = timeout {
//!             match tokio::time::timeout(timeout, self.stream.read(buf)).await {
//!                 Ok(x) => Ok(Some(x?)),
//!                 Err(_) => Ok(None),
//!             }
//!         }
//!         else {
//!             self.stream.read(buf).await.map(Some)
//!         }
//!     }
//!
//!     fn now(&mut self) -> Instant {
//!         Instant::from_duration_since_epoch(tokio::time::Instant::now() - self.epoch)
//!     }
//! }
//!
//! async fn run_client<P: Platform>(client: &mut NanoClient<'_>, platform: &mut P) -> Result<(), Error<P>> {
//!     let start_time = tokio::time::Instant::now();
//!     loop {
//!         let tick_result = client.next_notification(platform).await?;
//!         // handle notification
//!         // This needs to be stored in a seperate variable,
//!         // otherwise the borrow-checker will complain!
//!         let tick_result = tick_result.complete();
//!         client.complete_notification(platform, tick_result).await?;
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     let options = ConnectOptions {
//!         client_id: "umqtt-basic-client",
//!         keep_alive: 10,
//!         clean_session: true,
//!         ..Default::default()
//!     };
//!
//!     let mut tx_buffer = Box::new([0u8; 1024]);
//!     let mut rx_buffer = Box::new([0u8; 1024]);
//!     let mut platform = TokioPlatform {
//!         stream: TcpStream::connect(("test.mosquitto.org", 1883)).await.unwrap(),
//!         epoch: tokio::time::Instant::now(),
//!     };
//!     let mut client = NanoClient::new(tx_buffer.as_mut_slice(), rx_buffer.as_mut_slice());
//!     loop {
//!         let _connack = client.connect(&mut platform, &options).await;
//!         run_client(&mut client, &mut platform).await;
//!         tokio::time::sleep(Duration::from_secs(20)).await;
//!     }
//! }
//! ```

mod buffer;
mod notification;
mod platform;

use crate::packetview::connack::{ConnAck, ConnectReturnCode};
use crate::packetview::connect::ConnectOptions;
use crate::packetview::ping::PingReq;
use crate::packetview::puback::PubAck;
use crate::packetview::pubcomp::PubComp;
use crate::packetview::publish::{OutPublish, PayloadWriter, TopicWriter};
use crate::packetview::pubrec::PubRec;
use crate::packetview::subscribe::SubscribeWriter;
use crate::packetview::{QoS, WriteError, write_remaining_length};
use crate::transport_client::{Notification, TransportClient};
use core::fmt::{Debug, Formatter};

use crate::packetview::borrowed_buf::BorrowedBuf;
use buffer::Buffer;
pub use notification::*;
pub use platform::*;

/// The error type used by [`NanoClient`]
pub enum Error<P: Platform> {
    ConnectionClosed,
    UnexpectedPacket,
    IO(P::Error),
    ReadError(crate::packetview::Error),
    WriteError(crate::packetview::WriteError),
}

impl<P> Debug for Error<P>
where
    P: Platform,
    P::Error: Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::ConnectionClosed => write!(f, "ConnectionClosed"),
            Error::UnexpectedPacket => write!(f, "UnexpectedPacket"),
            Error::IO(x) => write!(f, "IO({:?})", x),
            Error::ReadError(x) => write!(f, "ReadError({:?})", x),
            Error::WriteError(x) => write!(f, "WriteError({:?})", x),
        }
    }
}

/// A nano MQTT client
///
/// This client is very small and opinionated client:
///
///   * Only subscribes to a pre-defined set of subscriptions.
///   * Cannot change subscriptions after creation.
///   * No session state handling - this is handled by the user.
///   * Doesn't keep track of in-flight packet ids - this is handled by the user.
///   * Doesn't keep track of unacked publishes - this is handled by the user.
///
pub struct NanoClient<'a> {
    transport_client: TransportClient,
    tx_buffer: &'a mut [u8],
    rx_buffer: Buffer<'a>,
}

impl<'a> NanoClient<'a> {
    async fn write_buffer<P: Platform>(
        transport_client: &mut TransportClient,
        platform: &mut P,
        buffer: &[u8],
    ) -> Result<(), Error<P>> {
        if transport_client.is_disconnected() {
            return Err(Error::ConnectionClosed);
        }
        match platform.write_all(buffer).await {
            Ok(()) => {
                transport_client.on_packet_sent(platform.now());
                Ok(())
            }
            Err(e) => {
                transport_client.on_transport_closed();
                Err(Error::IO(e))
            }
        }
    }

    async fn write_all<P: Platform>(&mut self, plat: &mut P, size: usize) -> Result<(), Error<P>> {
        Self::write_buffer(&mut self.transport_client, plat, &self.tx_buffer[..size]).await
    }

    async fn read_some_with_timeout<P: Platform>(
        &mut self,
        platform: &mut P,
        timeout: Option<core::time::Duration>,
    ) -> Result<Option<usize>, Error<P>> {
        match platform
            .read_some(self.rx_buffer.unused_slice_mut(), timeout)
            .await
        {
            Ok(None) => Ok(None),
            Ok(Some(n)) => {
                if n == 0 {
                    self.transport_client.on_transport_closed();
                    return Err(Error::ConnectionClosed);
                }
                self.rx_buffer.advance(n);
                Ok(Some(n))
            }
            Err(e) => {
                self.transport_client.on_transport_closed();
                Err(Error::IO(e))
            }
        }
    }

    async fn read_connack<P: Platform>(&mut self, platform: &mut P) -> Result<ConnAck, Error<P>> {
        loop {
            let read_result = self.read_some_with_timeout(platform, None).await?.unwrap();
            if read_result == 0 {
                return Err(Error::ConnectionClosed);
            }
            match self
                .transport_client
                .on_bytes_received(self.rx_buffer.used_slice())
            {
                Err(e) => return Err(Error::ReadError(e)),
                Ok(None) => {}
                Ok(Some((notif, taken))) => match notif {
                    Notification::ConnAck(ack) => {
                        self.rx_buffer.remove_prefix(taken);
                        return Ok(ack);
                    }
                    _ => return Err(Error::UnexpectedPacket),
                },
            }
        }
    }

    async fn write_subscribe<P, S, SE>(
        &mut self,
        platform: &mut P,
        subscriptions: S,
    ) -> Result<(), Error<P>>
    where
        P: Platform,
        S: FnOnce(&mut SubscribeWriter) -> Result<(), SE>,
        WriteError: From<SE>,
    {
        let mut borrowed_buf = BorrowedBuf::new(self.tx_buffer);
        subscriptions(&mut SubscribeWriter(&mut borrowed_buf))
            .map_err(|x| Error::WriteError(x.into()))?;
        let mut header = [0; 7];
        header[0] = 0x82;
        let remaining_len_len = write_remaining_length(
            &mut crate::packetview::cursor::WriteCursor::new(&mut header[1..]),
            borrowed_buf.len() + 2,
        )
        .map_err(Error::WriteError)?;
        header[remaining_len_len + 1] = 0;
        header[remaining_len_len + 2] = 1;

        Self::write_buffer(
            &mut self.transport_client,
            platform,
            &header[..remaining_len_len + 3],
        )
        .await?;
        let payload_len = borrowed_buf.len();
        Self::write_buffer(
            &mut self.transport_client,
            platform,
            &self.tx_buffer[..payload_len],
        )
        .await?;

        Ok(())
    }

    /// Create a new [`NanoClient`]
    ///
    /// # Arguments
    ///
    ///   * `platform` - Platform-specific functionality needed by the client.
    ///   * `tx_buffer` - Buffer to use when sending data.
    ///   * `rx_buffer` - Buffer to use when receiving data.
    ///
    /// # Returns
    ///
    /// A new [`NanoClient`]
    ///
    pub fn new(tx_buffer: &'a mut [u8], rx_buffer: &'a mut [u8]) -> Self {
        Self {
            transport_client: TransportClient::new(rx_buffer.len()),
            tx_buffer,
            rx_buffer: Buffer::new(rx_buffer),
        }
    }

    /// Connect to the MQTT broker.
    ///
    /// The [`Platform`] is expected to know which broker to connect to.
    ///
    /// # Arguments
    ///
    ///   * `platform` - The platform instance to use.
    ///   * `connect_options` - The options to use when connecting and authenticating with the broker.
    ///
    /// # Returns
    ///
    /// A result containing either the [`ConnAck`] response from the server, or an error if
    /// some error ocurred.
    ///
    /// **Note:** The [`ConnAck`] might report an error from the server. In that case the client
    /// assumes that the transport has closed as well.
    pub async fn connect<P: Platform>(
        &mut self,
        platform: &mut P,
        connect_options: &ConnectOptions<'_>,
    ) -> Result<ConnAck, Error<P>> {
        self.transport_client.on_transport_opened(connect_options);
        self.rx_buffer.reset();
        let out_size = connect_options
            .as_connect()
            .write(self.tx_buffer)
            .map_err(|e| Error::WriteError(e))?;
        self.write_all(platform, out_size).await?;
        let connack = self.read_connack(platform).await?;
        if connack.code != ConnectReturnCode::Success {
            self.transport_client.on_transport_closed();
        }

        Ok(connack)
    }

    /// Connect to the MQTT broker.
    ///
    /// The [`Platform`] is expected to know which broker to connect to.
    ///
    /// # Arguments
    ///
    ///   * `platform` - The platform to use.
    ///   * `connect_options` - The options to use when connecting and authenticating with the broker.
    ///   * `subscriptions` - A closure used to format all the subscriptions.
    ///
    /// # Returns
    ///
    /// A result containing either the [`ConnAck`] response from the server, or an error if
    /// some error ocurred.
    ///
    /// **Note:** The [`ConnAck`] might report an error from the server. In that case the client
    /// assumes that the transport has closed as well.
    pub async fn connect_subscribe<P, F, FE>(
        &mut self,
        platform: &mut P,
        connect_options: &ConnectOptions<'_>,
        subscriptions: F,
    ) -> Result<ConnAck, Error<P>>
    where
        P: Platform,
        F: FnOnce(&mut SubscribeWriter) -> Result<(), FE>,
        WriteError: From<FE>,
    {
        let connack = self.connect(platform, connect_options).await?;
        if connack.code.is_success() {
            self.write_subscribe(platform, subscriptions).await?;
        }
        Ok(connack)
    }

    /// Read data until next notification
    ///
    /// Reads data from the transport until either of the following is true:
    ///
    ///   * A new packet is received - the packet is returned to the sender.
    ///   * A ping should be sent
    ///   * An error ocurrs.
    ///
    /// # Arguments
    ///
    ///   * `platform` - The platform to use.
    ///
    pub async fn next_notification<'b, P>(
        &'b mut self,
        platform: &mut P,
    ) -> Result<ClientNotification<'b>, Error<P>>
    where
        'a: 'b,
        P: Platform,
    {
        if self.is_disconnected() {
            return Err(Error::ConnectionClosed);
        }
        loop {
            let now = platform.now();
            let next_ping = self.transport_client.next_ping_in(now);
            let Some(_read_result) = self.read_some_with_timeout(platform, next_ping).await? else {
                return Ok(ClientNotification::SendPing);
            };
            if crate::packetview::check(self.rx_buffer.used_slice(), self.rx_buffer.capacity())
                .is_ok()
            {
                break;
            }
        }

        match self
            .transport_client
            .on_bytes_received(self.rx_buffer.used_slice())
        {
            Err(e) => Err(Error::ReadError(e)),
            Ok(None) => Err(Error::ReadError(crate::packetview::Error::MalformedPacket)),
            Ok(Some((notif, taken))) => Ok(ClientNotification::TransportNotification(
                TransportNotification {
                    notification: notif,
                    taken,
                },
            )),
        }
    }

    /// User calls this when a notification is done and should be completed.
    ///
    /// The [`NotificationCompletion`] is obtained by calling one of the [`ClientNotification::complete`]
    /// functions.
    ///
    /// Failure to call this will cause the same notification to be re-received over and over again.
    ///
    /// This may cause the client to write data to the broker, so this is **not** cancellation safe.
    ///
    /// # Arguments
    ///
    ///   * `platform` - The platform to use.
    ///   * `completion` - The notification completion to complete.
    ///
    pub async fn complete_notification<P: Platform>(
        &mut self,
        platform: &mut P,
        completion: NotificationCompletion,
    ) -> Result<(), Error<P>> {
        if completion.taken > 0 {
            self.rx_buffer.remove_prefix(completion.taken);
        }
        match completion.action {
            NotificationAction::None => Ok(()),
            NotificationAction::PingReq => self.send_ping_request(platform).await,
            NotificationAction::PubAck(packet_id) => self.send_pub_ack(platform, packet_id).await,
            NotificationAction::PubRec(packet_id) => self.send_pub_rec(platform, packet_id).await,
            NotificationAction::PubComp(packet_id) => self.send_pub_comp(platform, packet_id).await,
        }
    }

    /// Send a ping request.
    ///
    /// This is usually done from [`complete_notification`] but can be done manually if for instance
    /// no keep alive is configured.
    ///
    /// # Arguments
    ///
    ///   * `platform` - The platform to use.
    ///
    pub async fn send_ping_request<P: Platform>(
        &mut self,
        platform: &mut P,
    ) -> Result<(), Error<P>> {
        let out_size = PingReq
            .write(self.tx_buffer)
            .map_err(|e| Error::WriteError(e))?;
        self.write_all(platform, out_size).await?;
        Ok(())
    }

    /// Send an MQTT PubAck message with a given packet id.
    ///
    /// # Arguments
    ///
    ///   * `platform` - The platform to use.
    ///   * `packet_id` - The ID of the publish to acknowledge.
    ///
    pub async fn send_pub_ack<P: Platform>(
        &mut self,
        platform: &mut P,
        packet_id: u16,
    ) -> Result<(), Error<P>> {
        let out_size = PubAck::new(packet_id)
            .write(self.tx_buffer)
            .map_err(|e| Error::WriteError(e))?;
        self.write_all(platform, out_size).await
    }

    /// Send an MQTT PubRec message with a given packet id.
    ///
    /// # Arguments
    ///
    ///   * `platform` - The platform to use.
    ///   * `packet_id` - The ID of the publish to acknowledge.
    ///
    pub async fn send_pub_rec<P: Platform>(
        &mut self,
        platform: &mut P,
        packet_id: u16,
    ) -> Result<(), Error<P>> {
        let out_size = PubRec::new(packet_id)
            .write(self.tx_buffer)
            .map_err(|e| Error::WriteError(e))?;
        self.write_all(platform, out_size).await
    }

    /// Send an MQTT PubComp message with a given packet id.
    ///
    /// # Arguments
    ///
    ///   * `platform` - The platform to use.
    ///   * `packet_id` - The ID of the publish to acknowledge.
    ///
    pub async fn send_pub_comp<P: Platform>(
        &mut self,
        platform: &mut P,
        packet_id: u16,
    ) -> Result<(), Error<P>> {
        let out_size = PubComp::new(packet_id)
            .write(self.tx_buffer)
            .map_err(|e| Error::WriteError(e))?;
        self.write_all(platform, out_size).await
    }

    /// Send an MQTT publish message with the given flags.
    ///
    /// Topic and payload are not given as buffers, instead
    /// they are functions that can be used to directly write topic and payload
    /// to the end-buffer.
    ///
    /// # Arguments
    ///
    ///   * `platform` - The platform to use.
    ///   * `publish` - Flags for the publish message.
    ///   * `topic` - Function that writes the topic.
    ///   * `payload` - Function that writes the payload.
    ///
    /// # Example
    ///
    /// ```no_run
    /// ```
    pub async fn send_publish<P, T, D, TE, DE>(
        &mut self,
        platform: &mut P,
        publish: OutPublish,
        topic: T,
        payload: D,
    ) -> Result<(), Error<P>>
    where
        P: Platform,
        T: FnOnce(&mut TopicWriter) -> Result<(), TE>,
        D: FnOnce(&mut PayloadWriter) -> Result<(), DE>,
        WriteError: From<TE>,
        WriteError: From<DE>,
    {
        let written = publish
            .write(topic, payload, self.tx_buffer)
            .map_err(|e| Error::WriteError(e))?;
        Self::write_buffer(&mut self.transport_client, platform, written).await
    }

    /// Send an MQTT publish message with QoS 0.
    ///
    /// See [`send_publish`] for details.
    ///
    pub async fn send_publish_qos0<P, T, D, TE, DE>(
        &mut self,
        platform: &mut P,
        topic: T,
        payload: D,
    ) -> Result<(), Error<P>>
    where
        P: Platform,
        T: FnOnce(&mut TopicWriter) -> Result<(), TE>,
        D: FnOnce(&mut PayloadWriter) -> Result<(), DE>,
        WriteError: From<TE>,
        WriteError: From<DE>,
    {
        self.send_publish(
            platform,
            OutPublish {
                pkid: 0,
                qos: QoS::AtMostOnce,
                retain: false,
                dup: false,
            },
            topic,
            payload,
        )
        .await
    }

    /// Send an MQTT publish message with QoS 1.
    ///
    /// See [`send_publish`] for details.
    ///
    pub async fn send_publish_qos1<P, T, D, TE, DE>(
        &mut self,
        platform: &mut P,
        packet_id: u16,
        topic: T,
        payload: D,
    ) -> Result<(), Error<P>>
    where
        P: Platform,
        T: FnOnce(&mut TopicWriter) -> Result<(), WriteError>,
        D: FnOnce(&mut PayloadWriter) -> Result<(), WriteError>,
        WriteError: From<TE>,
        WriteError: From<DE>,
    {
        self.send_publish(
            platform,
            OutPublish {
                pkid: packet_id,
                dup: false,
                retain: false,
                qos: QoS::AtLeastOnce,
            },
            topic,
            payload,
        )
        .await
    }

    /// Check if the underlying transport client is disconnected.
    pub fn is_disconnected(&self) -> bool {
        self.transport_client.is_disconnected()
    }

    /// Convert the [`NanoClient`] to a [`TransportClient`].
    pub fn into_inner(self) -> TransportClient {
        self.transport_client
    }

    /// Create a [`NanoClient`] from a [`TransportClient`].
    pub fn from_transport_client(
        transport_client: TransportClient,
        tx_buffer: &'a mut [u8],
        rx_buffer: &'a mut [u8],
    ) -> Self {
        Self {
            transport_client,
            tx_buffer,
            rx_buffer: Buffer::new(rx_buffer),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::nano_client::{NanoClient, Platform};
    use crate::packetview::QoS;
    use crate::packetview::connect::ConnectOptions;
    use crate::time::Instant;
    use alloc::boxed::Box;
    use alloc::collections::VecDeque;
    use alloc::rc::Rc;
    use alloc::vec::Vec;
    use core::cell::RefCell;
    use core::ops::Deref;
    use core::time::Duration;

    #[derive(Default)]
    struct TestPlatformData {
        packets_written: Vec<Box<[u8]>>,
        packets_to_read: VecDeque<Box<[u8]>>,
    }

    struct TestPlatform {
        data: Rc<RefCell<TestPlatformData>>,
    }

    impl TestPlatform {
        fn new() -> Self {
            Self {
                data: Rc::new(RefCell::new(TestPlatformData::default())),
            }
        }
    }

    impl Platform for TestPlatform {
        type Error = ();

        async fn write_all(&mut self, buf: &[u8]) -> Result<(), Self::Error> {
            self.data.borrow_mut().packets_written.push(buf.into());
            Ok(())
        }

        async fn read_some(
            &mut self,
            buf: &mut [u8],
            _timeout: Option<Duration>,
        ) -> Result<Option<usize>, Self::Error> {
            let Some(next_packet) = self.data.borrow_mut().packets_to_read.pop_front() else {
                return Ok(None);
            };

            buf[0..next_packet.len()].copy_from_slice(next_packet.as_ref());
            Ok(Some(next_packet.len()))
        }

        fn now(&mut self) -> Instant {
            Instant::from_seconds_since_epoch(0)
        }
    }

    #[test]
    fn disconnected_on_creation() {
        let mut platform = TestPlatform::new();
        let mut tx_buffer = [0u8; 256];
        let mut rx_buffer = [0u8; 256];
        let mut client = NanoClient::new(&mut tx_buffer, &mut rx_buffer);
        assert!(client.is_disconnected());

        let res = spin_on::spin_on(client.next_notification(&mut platform));
        assert!(res.is_err());
        let res = spin_on::spin_on(
            client.send_publish_qos0(&mut platform, |x| x.add_str("/test"), |x| x.add_slice(b"hello")),
        );
        assert!(res.is_err());
        let res = spin_on::spin_on(client.send_ping_request(&mut platform));
        assert!(res.is_err());
        let res = spin_on::spin_on(client.send_pub_comp(&mut platform, 2));
        assert!(res.is_err());
        let res = spin_on::spin_on(client.send_pub_ack(&mut platform, 2));
        assert!(res.is_err());
        let res = spin_on::spin_on(client.send_pub_rec(&mut platform, 2));
        assert!(res.is_err());
    }

    #[test]
    fn no_subscribe_on_empty_topic_filter_list() {
        let mut platform = TestPlatform::new();
        let data = platform.data.clone();
        data.borrow_mut()
            .packets_to_read
            .push_back(Box::new([0x20, 2, 0, 0])); // CONNACK.
        let mut tx_buffer = [0u8; 256];
        let mut rx_buffer = [0u8; 256];
        let mut client = NanoClient::new(&mut tx_buffer, &mut rx_buffer);

        let connect_options = ConnectOptions {
            keep_alive: 10,
            clean_session: true,
            client_id: "test",
            last_will: None,
            login: None,
        };

        let _res = spin_on::spin_on(client.connect(&mut platform, &connect_options)).unwrap();
        assert_eq!(data.borrow().packets_written.len(), 1);
    }

    #[test]
    fn subscribe_packet() {
        let mut platform = TestPlatform::new();
        let data = platform.data.clone();
        data.borrow_mut()
            .packets_to_read
            .push_back(Box::new([0x20, 2, 0, 0])); // CONNACK.
        let mut tx_buffer = [0u8; 256];
        let mut rx_buffer = [0u8; 256];
        let mut client = NanoClient::new(&mut tx_buffer, &mut rx_buffer);

        let connect_options = ConnectOptions {
            keep_alive: 10,
            clean_session: true,
            client_id: "test",
            last_will: None,
            login: None,
        };

        let _res = spin_on::spin_on(
            client.connect_subscribe(&mut platform, &connect_options, |x| x.add_str("hello", QoS::AtMostOnce)),
        )
        .unwrap();
        assert_eq!(data.borrow().packets_written[1].deref(), [0x82, 10, 0, 1]);
        assert_eq!(
            data.borrow().packets_written[2].deref(),
            b"\x00\x05hello\x00"
        );
    }
}
