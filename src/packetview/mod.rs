use crate::packetview::cursor::{Cursor, WriteCursor};

pub(crate) mod cursor;

pub mod borrowed_buf;
pub mod connack;
pub mod connect;
pub mod disconnect;
pub mod packet;
pub mod ping;
pub mod puback;
pub mod pubcomp;
pub mod publish;
pub mod pubrec;
pub mod pubrel;
pub mod suback;
pub mod subscribe;
pub mod topic_iterator;
pub mod unsuback;
pub mod unsubscribe;

#[derive(Eq, PartialEq, Clone, Debug)]
pub enum Error {
    NeedMoreData(usize),
    PayloadSizeLimitExceeded(usize),
    InvalidPacketType(u8),
    MalformedRemainingLength,
    MalformedPacket,
    EmptySubscription,
    BadUtf8,
    InvalidProtocol,
    InvalidProtocolLevel(u8),
    InvalidQoS(u8),
    InvalidConnectReturnCode(u8),
    PacketIdZero,
    UnsupportedPacket,
    InvalidSubscribeReasonCode(u8),
    UnexpectedPacket(PacketType),
}

#[derive(Eq, PartialEq, Clone, Debug)]
pub enum WriteError {
    MalformedPacket,
    NotEnoughCapacity,
    PayloadTooLong,
}

impl From<core::fmt::Error> for WriteError {
    fn from(_value: core::fmt::Error) -> Self {
        Self::NotEnoughCapacity
    }
}

/// Quality of service
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Default)]
pub enum QoS {
    #[default]
    AtMostOnce = 0,
    AtLeastOnce = 1,
    ExactlyOnce = 2,
}

/// Maps a number to QoS
pub fn qos(num: u8) -> Result<QoS, Error> {
    match num {
        0 => Ok(QoS::AtMostOnce),
        1 => Ok(QoS::AtLeastOnce),
        2 => Ok(QoS::ExactlyOnce),
        qos => Err(Error::InvalidQoS(qos)),
    }
}

/// Protocol type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    V4,
    V5,
}

/// MQTT packet type
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketType {
    Connect = 1,
    ConnAck,
    Publish,
    PubAck,
    PubRec,
    PubRel,
    PubComp,
    Subscribe,
    SubAck,
    Unsubscribe,
    UnsubAck,
    PingReq,
    PingResp,
    Disconnect,
}

/// Packet type from a byte
///
/// ```text
///          7                          3                          0
///          +--------------------------+--------------------------+
/// byte 1   | MQTT Control Packet Type | Flags for each type      |
///          +--------------------------+--------------------------+
///          |         Remaining Bytes Len  (1/2/3/4 bytes)        |
///          +-----------------------------------------------------+
///
/// <https://docs.oasis-open.org/mqtt/mqtt/v3.1.1/os/mqtt-v3.1.1-os.html#_Toc385349207>
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd)]
pub struct FixedHeader {
    /// First byte of the stream. Used to identify packet types and
    /// several flags
    byte1: u8,
    /// Length of fixed header. Byte 1 + (1..4) bytes. So fixed header
    /// len can vary from 2 bytes to 5 bytes
    /// 1..4 bytes are variable length encoded to represent remaining length
    fixed_header_len: usize,
    /// Remaining length of the packet. Doesn't include fixed header bytes
    /// Represents variable header + payload size
    remaining_len: usize,
}

impl FixedHeader {
    pub fn new(byte1: u8, remaining_len_len: usize, remaining_len: usize) -> FixedHeader {
        FixedHeader {
            byte1,
            fixed_header_len: remaining_len_len + 1,
            remaining_len,
        }
    }

    pub fn packet_type(&self) -> Result<PacketType, Error> {
        let num = self.byte1 >> 4;
        match num {
            1 => Ok(PacketType::Connect),
            2 => Ok(PacketType::ConnAck),
            3 => Ok(PacketType::Publish),
            4 => Ok(PacketType::PubAck),
            5 => Ok(PacketType::PubRec),
            6 => Ok(PacketType::PubRel),
            7 => Ok(PacketType::PubComp),
            8 => Ok(PacketType::Subscribe),
            9 => Ok(PacketType::SubAck),
            10 => Ok(PacketType::Unsubscribe),
            11 => Ok(PacketType::UnsubAck),
            12 => Ok(PacketType::PingReq),
            13 => Ok(PacketType::PingResp),
            14 => Ok(PacketType::Disconnect),
            _ => Err(Error::InvalidPacketType(num)),
        }
    }

    /// Returns the size of full packet (fixed header + variable header + payload)
    /// Fixed header is enough to get the size of a frame in the stream
    pub fn frame_length(&self) -> usize {
        self.fixed_header_len + self.remaining_len
    }
}

/// Checks if the stream has enough bytes to frame a packet and returns fixed header
/// only if a packet can be framed with existing bytes in the `stream`.
/// The passed stream doesn't modify parent stream's cursor. If this function
/// returned an error, next `check` on the same parent stream is forced start
/// with cursor at 0 again (Iter is owned. Only Iter's cursor is changed internally)
pub fn check(data: &[u8], max_packet_size: usize) -> Result<FixedHeader, Error> {
    // Create fixed header if there are enough bytes in the stream
    // to frame full packet
    let stream_len = data.len();
    let fixed_header = parse_fixed_header(data)?;

    // Don't let rogue connections attack with huge payloads.
    // Disconnect them before reading all that data
    if fixed_header.remaining_len > max_packet_size {
        return Err(Error::PayloadSizeLimitExceeded(fixed_header.remaining_len));
    }

    // If the current call fails due to insufficient bytes in the stream,
    // after calculating remaining length, we extend the stream
    let frame_length = fixed_header.frame_length();
    if stream_len < frame_length {
        return Err(Error::NeedMoreData(frame_length - stream_len));
    }

    Ok(fixed_header)
}

/// Parses fixed header
fn parse_fixed_header(data: &[u8]) -> Result<FixedHeader, Error> {
    // At least 2 bytes are necessary to frame a packet
    let stream_len = data.len();
    if stream_len < 2 {
        return Err(Error::NeedMoreData(2 - stream_len));
    }

    let mut stream = data.iter();

    let byte1 = stream.next().unwrap();
    let (len_len, len) = length(stream)?;

    Ok(FixedHeader::new(*byte1, len_len, len))
}

/// Parses variable byte integer in the stream and returns the length
/// and number of bytes that make it. Used for remaining length calculation
/// as well as for calculating property lengths
fn length(stream: core::slice::Iter<u8>) -> Result<(usize, usize), Error> {
    let mut len: usize = 0;
    let mut len_len = 0;
    let mut done = false;
    let mut shift = 0;

    // Use continuation bit at position 7 to continue reading next
    // byte to frame 'length'.
    // Stream 0b1xxx_xxxx 0b1yyy_yyyy 0b1zzz_zzzz 0b0www_wwww will
    // be framed as number 0bwww_wwww_zzz_zzzz_yyy_yyyy_xxx_xxxx
    for byte in stream {
        len_len += 1;
        let byte = *byte as usize;
        len += (byte & 0x7F) << shift;

        // stop when continue bit is 0
        done = (byte & 0x80) == 0;
        if done {
            break;
        }

        shift += 7;

        // Only a max of 4 bytes allowed for remaining length
        // more than 4 shifts (0, 7, 14, 21) implies bad length
        if shift > 21 {
            return Err(Error::MalformedRemainingLength);
        }
    }

    // Not enough bytes to frame remaining length. wait for
    // one more byte
    if !done {
        return Err(Error::NeedMoreData(1));
    }

    Ok((len_len, len))
}

/// After collecting enough bytes to frame a packet (packet's frame())
/// , It's possible that content itself in the stream is wrong. Like expected
/// packet id or qos not being present. In cases where `read_mqtt_string` or
/// `read_mqtt_bytes` exhausted remaining length but packet framing expects to
/// parse qos next, these pre checks will prevent panics
fn read_u8(stream: &mut Cursor) -> Result<u8, Error> {
    if stream.0.is_empty() {
        return Err(Error::MalformedPacket);
    }

    Ok(stream.take_u8())
}

/// After collecting enough bytes to frame a packet (packet's frame())
/// , It's possible that content itself in the stream is wrong. Like expected
/// packet id or qos not being present. In cases where `read_mqtt_string` or
/// `read_mqtt_bytes` exhausted remaining length but packet framing expects to
/// parse qos next, these pre checks will prevent `bytes` crashes
fn read_u16(stream: &mut Cursor) -> Result<u16, Error> {
    if stream.0.len() < 2 {
        return Err(Error::MalformedPacket);
    }

    Ok(stream.take_u16())
}

/// Reads a series of bytes with a length from a byte stream
fn read_mqtt_bytes<'a>(stream: &mut Cursor<'a>) -> Result<&'a [u8], Error> {
    let len = read_u16(stream)? as usize;

    // Prevent attacks with wrong remaining length. This method is used in
    // `packet.assembly()` with (enough) bytes to frame packet. Ensures that
    // reading variable len string or bytes doesn't cross promised boundary
    // with `read_fixed_header()`
    if len > stream.0.len() {
        return Err(Error::MalformedPacket);
    }

    Ok(stream.split_to(len))
}

/// Reads a string from bytes stream
fn read_mqtt_string<'a>(stream: &mut Cursor<'a>) -> Result<&'a str, Error> {
    let s = read_mqtt_bytes(stream)?;
    match core::str::from_utf8(s) {
        Ok(v) => Ok(v),
        Err(_e) => Err(Error::BadUtf8),
    }
}

/// Serializes bytes to stream (including length)
fn write_mqtt_bytes(stream: &mut WriteCursor, bytes: &[u8]) -> Result<(), WriteError> {
    stream.put_u16(bytes.len() as u16)?;
    stream.put_slice(bytes)
}

/// Serializes a string to stream
fn write_mqtt_string(stream: &mut WriteCursor, string: &str) -> Result<(), WriteError> {
    write_mqtt_bytes(stream, string.as_bytes())
}

/// Writes remaining length to stream and returns number of bytes for remaining length
pub(crate) fn write_remaining_length(
    stream: &mut WriteCursor,
    len: usize,
) -> Result<usize, WriteError> {
    if len > 268_435_455 {
        return Err(WriteError::PayloadTooLong);
    }

    let mut done = false;
    let mut x = len;
    let mut count = 0;

    while !done {
        let mut byte = (x % 128) as u8;
        x /= 128;
        if x > 0 {
            byte |= 128;
        }

        stream.put_u8(byte)?;
        count += 1;
        done = x == 0;
    }

    Ok(count)
}
