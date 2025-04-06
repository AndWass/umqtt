use crate::packetview::connack::{ConnAck, ConnectReturnCode};
use crate::packetview::connect::{ConnectOptions};
use crate::packetview::Error;
use crate::packetview::packet::Packet;
use crate::packetview::puback::PubAck;
use crate::packetview::pubcomp::PubComp;
use crate::packetview::publish::Publish;
use crate::packetview::pubrec::PubRec;
use crate::packetview::pubrel::PubRel;
use crate::time::Instant;

pub enum Notification<'a> {
    ConnAck(ConnAck),
    Publish(Publish<'a>),
    PubAck(PubAck),
    PubRec(PubRec),
    PubRel(PubRel),
    PubComp(PubComp),
    /*SubAck(SubAck),
    UnsubAck(UnsubAck),*/
    PingResponse,
    Disconnected,
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
    ///   * `options`: The options to be used when connecting to the broker
    ///
    pub fn on_transport_opened(&mut self, options: &ConnectOptions) {
        self.keep_alive = core::time::Duration::from_secs(options.keep_alive.into());
        self.packet_send_at = Instant::from_seconds_since_epoch(0);
        self.state = State::TransportConnected;
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
    }

    /// Call when new bytes have been received
    ///
    /// This will attempt to parse a packet from the slice of bytes, and if successful
    /// it will return a parsed packet and the amount of bytes taken.
    ///
    /// When a packet is successfully parsed the upper layer should remove all the taken bytes
    /// from its buffer.
    ///
    /// # Example
    ///
    /// ```
    /// use umqtt::client::TransportClient;
    /// use umqtt::client::Notification;
    /// use umqtt::packetview::connect::ConnectOptions;
    /// let mut client = TransportClient::new(256);
    /// client.on_transport_opened(&ConnectOptions::default());
    /// let mut data = vec![0xD0, 0, 0xD0, 0];
    /// let (pingresp, taken) = client.on_bytes_received(data.as_slice()).unwrap().unwrap();
    /// assert_eq!(taken, 2);
    /// assert!(matches!(pingresp, Notification::PingResponse));
    /// data.drain(0..2);
    /// let (pingresp, taken) = client.on_bytes_received(data.as_slice()).unwrap().unwrap();
    /// assert_eq!(taken, 2);
    /// assert!(matches!(pingresp, Notification::PingResponse));
    /// data.drain(0..2);
    /// let res = client.on_bytes_received(data.as_slice()).unwrap();
    /// assert!(matches!(res, None));
    /// ```
    ///
    pub fn on_bytes_received<'a>(&mut self, data: &'a [u8]) -> Result<Option<(Notification<'a>, usize)>, Error> {
        if matches!(self.state, State::Disconnected) {
            return Ok(Some((Notification::Disconnected, 0)));
        }
        match Packet::read(data, self.max_packet_size) {
            Err(Error::NeedMoreData(_)) => Ok(None),
            Err(x) => {
                self.state = State::Disconnected;
                Err(x)
            },
            Ok((Packet::ConnAck(c), taken)) => {
                if c.code == ConnectReturnCode::Success {
                    self.state = State::Connected;
                    Ok(Some((Notification::ConnAck(c), taken)))
                }
                else {
                    self.state = State::Disconnected;
                    Ok(Some((Notification::Disconnected, 0)))
                }
            },
            Ok((Packet::Publish(publish), taken)) => Ok(Some((Notification::Publish(publish), taken))),
            Ok((Packet::PubAck(ack), taken)) => Ok(Some((Notification::PubAck(ack), taken))),
            Ok((Packet::PubRec(rec), taken)) => Ok(Some((Notification::PubRec(rec), taken))),
            Ok((Packet::PubRel(rel), taken)) => Ok(Some((Notification::PubRel(rel), taken))),
            Ok((Packet::PubComp(comp), taken)) => Ok(Some((Notification::PubComp(comp), taken))),
            /*Ok(Packet::SubAck(ack)) => Ok(Some(Notification::SubAck(ack))),
            Ok(Packet::UnsubAck(ack)) => Ok(Some(Notification::UnsubAck(ack))),*/
            Ok((Packet::PingResp, taken)) => Ok(Some((Notification::PingResponse, taken))),
            _ => {
                self.state = State::Disconnected;
                Err(Error::UnsupportedPacket)
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
