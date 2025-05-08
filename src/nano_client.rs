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
//! A basic example client is as follows:
//!
//! ```no_run
//! use std::time::Duration;
//! use tokio::io::{AsyncReadExt, AsyncWriteExt};
//! use tokio::net::TcpStream;
//! use umqtt::nano_client::{NanoClient, ClientNotification};
//! use umqtt::packetview::connect::ConnectOptions;
//! use umqtt::packetview::QoS;
//! use umqtt::packetview::subscribe::{SubscribeFilter};
//! use umqtt::time::Instant;
//!
//! struct Platform {
//!     stream: Option<TcpStream>,
//!     epoch: tokio::time::Instant,
//! }
//!
//! impl umqtt::nano_client::Platform for Platform {
//!     type Error = std::io::Error;
//!
//!     async fn connect_transport(&mut self) -> Result<(), Self::Error> {
//!         self.stream = Some(TcpStream::connect(("test.mosquitto.org", 1883)).await?);
//!         Ok(())
//!     }
//!
//!     async fn write_all(&mut self, buf: &[u8]) -> Result<(), Self::Error> {
//!         self.stream.as_mut().unwrap().write_all(buf).await
//!     }
//!
//!     async fn read_some(&mut self, buf: &mut [u8], timeout: Option<Duration>) -> Result<Option<usize>, Self::Error> {
//!         if let Some(timeout) = timeout {
//!             match tokio::time::timeout(timeout, self.stream.as_mut().unwrap().read(buf)).await {
//!                 Ok(x) => Ok(Some(x?)),
//!                 Err(_) => Ok(None),
//!             }
//!         }
//!         else {
//!             self.stream.as_mut().unwrap().read(buf).await.map(Some)
//!         }
//!     }
//!
//!     fn now(&mut self) -> Instant {
//!         Instant::from_duration_since_epoch(tokio::time::Instant::now() - self.epoch)
//!     }
//! }
//!
//! async fn run_client<P: umqtt::nano_client::Platform>(client: &mut NanoClient<'_, P>) -> Result<(), umqtt::nano_client::Error<P>> {
//!     let start_time = tokio::time::Instant::now();
//!     loop {
//!         let tick_result = client.next_notification().await?;
//!         // handle notification
//!         // This needs to be stored in a seperate variable, otherwise the borrow-checker will complain!
//!         let tick_result = tick_result.complete();
//!         client.complete_notification(tick_result).await?;
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
//!     let platform = Platform {
//!         stream: None,
//!         epoch: tokio::time::Instant::now(),
//!     };
//!     let subscriptions = [SubscribeFilter::new("/umqtt/#", QoS::AtLeastOnce)];
//!     let mut client = NanoClient::new(platform, tx_buffer.as_mut_slice(), rx_buffer.as_mut_slice(), &subscriptions);
//!     loop {
//!         let _connack = client.connect(&options).await;
//!         run_client(&mut client).await;
//!         tokio::time::sleep(Duration::from_secs(20)).await;
//!     }
//! }
//! ```

use core::fmt::{Debug, Formatter};
use crate::packetview::connack::ConnAck;
use crate::packetview::connect::ConnectOptions;
use crate::packetview::ping::PingReq;
use crate::packetview::puback::PubAck;
use crate::packetview::pubcomp::PubComp;
use crate::packetview::pubrec::PubRec;
use crate::packetview::QoS;
use crate::packetview::subscribe::{Subscribe, SubscribeFilter};
use crate::time::Instant;
use crate::transport_client::{Notification, TransportClient};

#[derive(Debug)]
pub struct TransportNotification<'a> {
    /// The underlying notification received from the transport client
    pub notification: Notification<'a>,
    /// The amount of data this notification takes in the RX buffer
    taken: usize,
}

/// The action to take when completing a notification
pub enum NotificationAction
{
    None,
    PingReq,
    PubAck(u16),
    PubRec(u16),
    PubComp(u16),
}

/// Each notification should be completed to ensure buffers are handled correctly
pub struct NotificationCompletion {
    taken: usize,
    action: NotificationAction,
}

impl NotificationCompletion {
    /// Create a `NotificationCompletion` with a specific action.
    pub fn with_action(tick_result: ClientNotification, action: NotificationAction) -> Self {
        Self {
            taken: tick_result.taken(),
            action
        }
    }
}

/// A notification returned from [`NanoClient::next_notification()`]
#[derive(Debug)]
pub enum ClientNotification<'a> {
    /// The transport has completed a notification.
    TransportNotification(TransportNotification<'a>),
    /// The client wants the user to send a ping.
    SendPing,
}

impl ClientNotification<'_> {
    fn taken(&self) -> usize {
        match self {
            ClientNotification::TransportNotification(x) => x.taken,
            ClientNotification::SendPing => 0,
        }
    }
    /// Complete a [`ClientNotification`] with a default completion token.
    ///
    /// The following mappings of notification are used:
    ///
    /// | Notification  | Action         |
    /// |---------------|----------------|
    /// | SendPing      | Send `PingReq` |
    /// | Publish QoS 0 | No action      |
    /// | Publish QoS 1 | Send `PubAck`  |
    /// | Publish QoS 2 | Send `PubRec`  |
    /// | PubRel        | Send `PubComp` |
    /// | All other     | No action      |
    ///
    pub fn complete(self) -> NotificationCompletion {
        match self {
            Self::SendPing => NotificationCompletion {
                taken: 0,
                action: NotificationAction::PingReq,
            },
            Self::TransportNotification(x) => {
                let taken = x.taken;
                let action = match x.notification {
                    Notification::Publish(publish) => {
                        match publish.qos {
                            QoS::AtMostOnce => NotificationAction::None,
                            QoS::AtLeastOnce => NotificationAction::PubAck(publish.pkid),
                            QoS::ExactlyOnce => NotificationAction::PubRec(publish.pkid),
                        }
                    },
                    Notification::PubRel(pubrel) => NotificationAction::PubComp(pubrel.pkid),
                    _ => NotificationAction::None
                };

                NotificationCompletion {
                    taken,
                    action
                }
            }
        }
    }

    /// Complete a [`ClientNotification`] with no action.
    ///
    pub fn complete_no_action(self) -> NotificationCompletion {
        NotificationCompletion {
            taken: self.taken(),
            action: NotificationAction::None,
        }
    }

    /// Complete a [`ClientNotification`] with a default completion token.
    ///
    /// The following mappings of notification are used:
    ///
    /// | Notification  | Action         |
    /// |---------------|----------------|
    /// | SendPing      | Send `PingReq` |
    /// | All other     | No action      |
    ///
    pub fn complete_no_pub_ack(self) -> NotificationCompletion {
        match self {
            ClientNotification::TransportNotification(x) => NotificationCompletion {
                taken: x.taken,
                action: NotificationAction::None,
            },
            ClientNotification::SendPing => NotificationCompletion {
                taken: 0,
                action: NotificationAction::PingReq,
            },
        }
    }
}

/// Trait that describes the platform that this client runs on.
#[allow(async_fn_in_trait)]
pub trait Platform {
    /// Platform-specific error for all the platform methods.
    type Error;
    /// Connect the underlying transport. If already connected it should reconnect again.
    async fn connect_transport(&mut self) -> Result<(), Self::Error>;
    /// Write all the data in `buf` to the underlying transport.
    ///
    /// # Arguments
    ///
    ///   * `buf` - Buffer of data to write.
    ///
    /// # Returns
    ///
    ///   * `Ok(())` - All data was written
    ///   * `Err(e)` - An error ocurred. The client will treat this as the transport has closed.
    ///
    async fn write_all(&mut self, buf: &[u8]) -> Result<(), Self::Error>;
    /// Read some data into `buf`.
    ///
    /// **Note:** This should be cancel-safe for correct operation of the `NanoClient`.
    ///
    /// Buffer doesn't have to be filled, it is perfectly valid to return as soon as any bytes has been
    /// read, or on a timeout.
    ///
    /// # Arguments
    ///
    ///   * `buf` - Buffer to store the read data
    ///   * `timeout` - Timeout when the read operation should be aborted if no data has been received.
    ///
    /// # Returns
    ///
    ///   * `Ok(None)` - Timeout has expired.
    ///   * `Ok(Some(0))` - The underlying transport has closed (EOF). The client will treat this as transport has closed.
    ///   * `Ok(Some(n))` - `n` bytes were transferred to `buf`.
    ///   * `Err(_)` - An error has ocurred. The client will treat this as transport has closed.
    ///
    async fn read_some(&mut self, buf: &mut [u8], timeout: Option<core::time::Duration>) -> Result<Option<usize>, Self::Error>;
    /// Get the current time
    ///
    /// Gets the current time since some unspecified epoch.
    fn now(&mut self) -> Instant;
}

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
            Error::WriteError(x) => write!(f, "WriteError({:?})", x)
        }
    }
}

struct Buffer<'a> {
    buffer: &'a mut [u8],
    used: usize,
}

impl<'a> Buffer<'a> {
    fn new(buffer: &'a mut [u8]) -> Self {
        Self { buffer, used: 0 }
    }

    fn reset(&mut self) {
        self.used = 0;
    }

    fn used_slice(&self) -> &[u8] {
        &self.buffer[..self.used]
    }

    fn unused_slice_mut(&mut self) -> &mut [u8] {
        &mut self.buffer[self.used..]
    }

    fn remove_prefix(&mut self, len: usize) {
        if len >= self.used {
            self.used = 0;
            return;
        }
        self.buffer.copy_within(len..self.used, 0);
        self.used -= len;
    }
}

pub struct NanoClient<'a, P> {
    platform: P,
    subscriptions: Subscribe<'a>,
    transport_client: TransportClient,
    tx_buffer: &'a mut [u8],
    rx_buffer: Buffer<'a>,
}

impl<'a, P: Platform> NanoClient<'a, P> {
    async fn connect_transport(&mut self) -> Result<(), Error<P>> {
        match self.platform.connect_transport().await {
            Ok(()) => Ok(()),
            Err(e) => Err(Error::IO(e)),
        }
    }

    async fn write_all(&mut self, size: usize) -> Result<(), Error<P>> {
        match self.platform.write_all(&self.tx_buffer[..size]).await {
            Ok(()) => {
                self.transport_client.on_packet_sent(self.platform.now());
                Ok(())
            },
            Err(e) => Err(Error::IO(e)),
        }
    }

    async fn read_some_with_timeout(&mut self, timeout: Option<core::time::Duration>) -> Result<Option<usize>, Error<P>> {
        match self.platform.read_some(self.rx_buffer.unused_slice_mut(), timeout).await {
            Ok(None) => Ok(None),
            Ok(Some(n)) => {
                if n == 0 {
                    self.transport_client.on_transport_closed();
                    return Err(Error::ConnectionClosed);
                }
                self.rx_buffer.used += n;
                Ok(Some(n))
            },
            Err(e) => {
                self.transport_client.on_transport_closed();
                Err(Error::IO(e))
            },
        }
    }

    pub async fn read_some(&mut self) -> Result<Option<usize>, Error<P>> {
        let timeout = self.transport_client.next_ping_in(self.platform.now());
        self.read_some_with_timeout(timeout).await
    }

    pub async fn read_connack(&mut self) -> Result<ConnAck, Error<P>> {
        loop {
            let read_result = self.read_some_with_timeout(None).await?.unwrap();
            if read_result == 0 {
                return Err(Error::ConnectionClosed);
            }
            match self.transport_client.on_bytes_received(self.rx_buffer.used_slice()) {
                Err(e) => return Err(Error::ReadError(e)),
                Ok(None) => {},
                Ok(Some((notif, taken))) => {
                    match notif {
                        Notification::ConnAck(ack) => {
                            self.rx_buffer.remove_prefix(taken);
                            return Ok(ack);
                        },
                        _ => return Err(Error::UnexpectedPacket),
                    }
                }
            }
        }
    }

    pub fn new(platform: P, tx_buffer: &'a mut[u8], rx_buffer: &'a mut [u8], subscriptions: &'a[SubscribeFilter<'a>]) -> Self {
        Self {
            subscriptions: Subscribe::new(1, subscriptions),
            transport_client: TransportClient::new(rx_buffer.len()),
            platform,
            tx_buffer,
            rx_buffer: Buffer::new(rx_buffer),
        }
    }

    pub async fn connect(&mut self, connect_options: &ConnectOptions<'_>) -> Result<ConnAck, Error<P>> {
        self.connect_transport().await?;
        self.transport_client.on_transport_opened(connect_options);
        self.rx_buffer.reset();
        let out_size = connect_options.as_connect().write(self.tx_buffer).map_err(|e| Error::WriteError(e))?;
        self.write_all(out_size).await?;
        let connack = self.read_connack().await?;
        let out_size = self.subscriptions.write(self.tx_buffer).map_err(|e| Error::WriteError(e))?;
        self.write_all(out_size).await?;
        Ok(connack)
    }

    pub async fn next_notification<'b>(&'b mut self) -> Result<ClientNotification<'b>, Error<P>>
    where
        'a: 'b
    {
        loop {
            let now = self.platform.now();
            let next_ping = self.transport_client.next_ping_in(now);
            let Some(_read_result) = self.read_some_with_timeout(next_ping).await? else {
                return Ok(ClientNotification::SendPing);
            };
            if crate::packetview::check(self.rx_buffer.used_slice(), self.rx_buffer.buffer.len()).is_ok() {
                break;
            }
        }

        match self.transport_client.on_bytes_received(self.rx_buffer.used_slice()) {
            Err(e) => Err(Error::ReadError(e)),
            Ok(None) => Err(Error::ReadError(crate::packetview::Error::MalformedPacket)),
            Ok(Some((notif, taken))) => {
                Ok(ClientNotification::TransportNotification(TransportNotification {
                    notification: notif,
                    taken,
                }))
            }
        }
    }

    pub async fn complete_notification(&mut self, completion: NotificationCompletion) -> Result<(), Error<P>> {
        if completion.taken > 0 {
            self.rx_buffer.remove_prefix(completion.taken);
        }
        match completion.action {
            NotificationAction::None => Ok(()),
            NotificationAction::PingReq => self.send_ping_request().await,
            NotificationAction::PubAck(packet_id) => self.send_pub_ack(packet_id).await,
            NotificationAction::PubRec(packet_id) => self.send_pub_rec(packet_id).await,
            NotificationAction::PubComp(packet_id) => self.send_pub_comp(packet_id).await,
        }
    }

    pub async fn send_ping_request(&mut self) -> Result<(), Error<P>> {
        let out_size = PingReq.write(self.tx_buffer).map_err(|e| Error::WriteError(e))?;
        self.write_all(out_size).await?;
        Ok(())
    }

    pub async fn send_pub_ack(&mut self, packet_id: u16) -> Result<(), Error<P>> {
        let out_size = PubAck::new(packet_id).write(self.tx_buffer).map_err(|e| Error::WriteError(e))?;
        self.write_all(out_size).await
    }

    pub async fn send_pub_rec(&mut self, packet_id: u16) -> Result<(), Error<P>> {
        let out_size = PubRec::new(packet_id).write(self.tx_buffer).map_err(|e| Error::WriteError(e))?;
        self.write_all(out_size).await
    }

    pub async fn send_pub_comp(&mut self, packet_id: u16) -> Result<(), Error<P>> {
        let out_size = PubComp::new(packet_id).write(self.tx_buffer).map_err(|e| Error::WriteError(e))?;
        self.write_all(out_size).await
    }
}

#[cfg(test)]
mod tests {
    use alloc::boxed::Box;
    use alloc::collections::VecDeque;
    use alloc::rc::Rc;
    use alloc::vec::Vec;
    use core::cell::RefCell;
    use core::time::Duration;
    use crate::nano_client::{NanoClient, Platform};
    use crate::time::Instant;

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
                data: Rc::new(RefCell::new(TestPlatformData::default()))
            }
        }
    }

    impl Platform for TestPlatform {
        type Error = ();

        async fn connect_transport(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }

        async fn write_all(&mut self, buf: &[u8]) -> Result<(), Self::Error> {
            self.data.borrow_mut().packets_written.push(buf.into());
            Ok(())
        }

        async fn read_some(&mut self, buf: &mut [u8], _timeout: Option<Duration>) -> Result<Option<usize>, Self::Error> {
            let Some(next_packet) = self.data.borrow_mut().packets_to_read.pop_front() else {
                return Ok(None);
            };

            (&mut buf[0..next_packet.len()]).copy_from_slice(next_packet.as_ref());
            Ok(Some(next_packet.len()))
        }

        fn now(&mut self) -> Instant {
            Instant::from_seconds_since_epoch(0)
        }
    }

    #[test]
    fn tick_one() {
        let mut rx_buffer = [0; 256];
        let mut tx_buffer = [0; 256];
        let platform = TestPlatform::new();
        let mut client = NanoClient::new(platform, &mut tx_buffer, &mut rx_buffer, &[]);
        let notif = spin_on::spin_on(client.next_notification()).unwrap();
        let completion = notif.complete();
        spin_on::spin_on(client.complete_notification(completion)).unwrap();
    }
}
