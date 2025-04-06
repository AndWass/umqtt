use crate::packetview::{qos, read_mqtt_string, read_u16, Error, FixedHeader, QoS};
use crate::packetview::cursor::Cursor;

/// Publish packet
#[derive(Clone, PartialEq, Eq)]
pub struct Publish<'a> {
    pub dup: bool,
    pub qos: QoS,
    pub retain: bool,
    pub topic: &'a str,
    pub pkid: u16,
    pub payload: &'a [u8],
}

impl core::fmt::Debug for Publish<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "Topic = {}, Qos = {:?}, Retain = {}, Pkid = {:?}, Payload Size = {}",
            self.topic,
            self.qos,
            self.retain,
            self.pkid,
            self.payload.len()
        )
    }
}

impl<'a> Publish<'a> {
    pub fn read_exact(fixed_header: FixedHeader, bytes: &'a [u8]) -> Result<Self, Error> {
        let qos = qos((fixed_header.byte1 & 0b0110) >> 1)?;
        let dup = (fixed_header.byte1 & 0b1000) != 0;
        let retain = (fixed_header.byte1 & 0b0001) != 0;

        let variable_header_index = fixed_header.fixed_header_len;
        let mut bytes = Cursor(bytes);
        bytes.advance(variable_header_index);
        let topic = read_mqtt_string(&mut bytes)?;

        // Packet identifier exists where QoS > 0
        let pkid = match qos {
            QoS::AtMostOnce => 0,
            QoS::AtLeastOnce | QoS::ExactlyOnce => read_u16(&mut bytes)?,
        };

        if qos != QoS::AtMostOnce && pkid == 0 {
            return Err(Error::PacketIdZero);
        }

        let publish = Publish {
            dup,
            retain,
            qos,
            pkid,
            topic,
            payload: bytes.as_slice(),
        };

        Ok(publish)
    }
}