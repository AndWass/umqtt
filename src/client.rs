use bytes::{Bytes, BytesMut};
use crate::mqttbytes::v4::{ConnAck, ConnectReturnCode, LastWill, Login, Packet, PubAck, Publish, SubAck, UnsubAck};
use crate::time::Instant;

pub enum Notification {
    ConnAck(ConnAck),
    Publish(Publish),
    PubAck(PubAck),
    SubAck(SubAck),
    UnsubAck(UnsubAck),
    PingResponse,
    Disconnected,
}

pub enum OutgoingData {
    Connect(Bytes)
}

impl OutgoingData {
    pub fn get_bytes(&self) -> &Bytes {
        match self {
            OutgoingData::Connect(x) => x,
        }
    }
}

impl AsRef<[u8]> for OutgoingData {
    fn as_ref(&self) -> &[u8] {
        self.get_bytes()
    }
}

#[derive(Debug, Default)]
pub struct ClientOptions {
    pub client_id: String,
    pub last_will: Option<LastWill>,
    pub login: Option<Login>,
    pub keep_alive: u16,
    pub clean_session: bool,
}

pub enum State {
    Disconnected,
    TransportConnected,
    Connected,
}

/// A low level client that aims at handling the transport aspect of a full MQTT client
///
/// This class handles the following aspects:
///   * Crafting the initial connect message based on MQTT options
///   * Buffering received bytes, assembling received packets when data is available
///   * Keeping track of when ping requests should be sent
///
/// For time-keeping a crate-specific [`Instant`] struct is used. This should be treated as a
/// monotonic clock that always moves forward in time (much like `std::time::Instant`).
///
pub struct TransportClient {
    bytes: BytesMut,
    keep_alive: core::time::Duration,
    state: State,
    max_packet_size: usize,
    packet_send_at: Instant,
}

impl TransportClient {
    const ZERO_KEEP_ALIVE: core::time::Duration = core::time::Duration::from_secs(0);
    /// Create a new transport client, with a maximum receive packet size set
    ///
    /// # Arguments
    ///
    ///   * `max_packet_size`: Maximum packet receive size
    ///
    /// # Returns
    ///
    /// A new [`TransportClient`].
    ///
    pub fn new(max_packet_size: usize) -> Self {
        Self {
            bytes: BytesMut::new(),
            keep_alive: core::time::Duration::from_secs(0),
            state: State::Disconnected,
            max_packet_size,
            packet_send_at: Instant::from_seconds_since_epoch(0),
        }
    }

    /// Signal to the client that a transport to a broker has been opened.
    ///
    /// This will craft a `Connect` message and encode that into a complete message that
    /// should be sent to the broker.
    ///
    /// # Arguments
    ///
    ///   * `options`: The options to use when connecting to the broker
    ///
    /// # Returns
    ///
    /// A complete `Connect` message to be sent to the broker.
    ///
    pub fn on_transport_opened(&mut self, options: &ClientOptions) -> Bytes {
        self.bytes.clear();
        self.keep_alive = core::time::Duration::from_secs(options.keep_alive.into());
        let mut connect = crate::mqttbytes::v4::Connect::new(options.client_id.as_str());
        connect.keep_alive = options.keep_alive;
        connect.clean_session = options.clean_session;
        connect.last_will = options.last_will.clone();
        connect.protocol = crate::mqttbytes::Protocol::V4;
        connect.login = options.login.clone();
        let mut buffer = BytesMut::new();
        let _written = connect.write(&mut buffer).unwrap();
        self.packet_send_at = Instant::from_seconds_since_epoch(0);
        buffer.freeze()
    }

    /// Tell the client that a packet has been sent.
    ///
    /// This should be called for every packet sent to the broker.
    ///
    /// # Arguments
    ///
    ///   * `now`: The time since some unspecified epoch.
    ///
    pub fn on_packet_sent(&mut self, now: Instant) {
        if self.keep_alive > Self::ZERO_KEEP_ALIVE {
            self.packet_send_at = now + self.keep_alive;
        }
    }

    /// Tell the client that the transport has been closed.
    ///
    pub fn on_transport_closed(&mut self) {
        self.state = State::Disconnected;
        self.bytes.clear();
    }

    /// Add received bytes to the transport client
    ///
    /// Bytes received on the transport are buffered. A client
    /// should typically call [`next_notification`] after bytes has been added.
    ///
    pub fn on_bytes_received(&mut self, data: &[u8]) {
        self.bytes.extend_from_slice(data);
    }

    /// Get the next pending notification based on the received bytes
    ///
    /// This should be called in a loop until `None` or [`Notification::Disconnected`] is returned.
    ///
    /// # Returns
    ///
    ///   * `None`: More bytes are needed
    /// The next notification based on the buffered received bytes.
    ///
    /// If the [`state()`] returns [`State::Disconnected`]
    pub fn next_notification(&mut self) -> Result<Option<Notification>, crate::mqttbytes::Error> {
        if matches!(self.state, State::Disconnected) {
            return Ok(Some(Notification::Disconnected));
        }
        match Packet::read(&mut self.bytes, self.max_packet_size) {
            Err(crate::mqttbytes::Error::InsufficientBytes(_)) => Ok(None),
            Err(x) => {
                self.state = State::Disconnected;
                Err(x)
            },
            Ok(Packet::ConnAck(c)) => {
                if c.code == ConnectReturnCode::Success {
                    self.state = State::Connected;
                    Ok(Some(Notification::ConnAck(c)))
                }
                else {
                    self.state = State::Disconnected;
                    Ok(Some(Notification::Disconnected))
                }
            },
            Ok(Packet::Publish(publish)) => Ok(Some(Notification::Publish(publish))),
            Ok(Packet::PubAck(ack)) => Ok(Some(Notification::PubAck(ack))),
            Ok(Packet::SubAck(ack)) => Ok(Some(Notification::SubAck(ack))),
            Ok(Packet::UnsubAck(ack)) => Ok(Some(Notification::UnsubAck(ack))),
            Ok(Packet::PingResp) => Ok(Some(Notification::PingResponse)),
            _ => {
                self.state = State::Disconnected;
                Err(crate::mqttbytes::Error::IncorrectPacketFormat)
            }
        }
    }

    pub fn next_ping_in(&self, now: Instant) -> Option<core::time::Duration> {
        if self.keep_alive > Self::ZERO_KEEP_ALIVE {
            Some(self.packet_send_at - now)
        }
        else {
            None
        }
    }
}
