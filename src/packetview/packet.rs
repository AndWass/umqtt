use crate::packetview::connack::ConnAck;
use crate::packetview::connect::Connect;
use crate::packetview::puback::PubAck;
use crate::packetview::pubcomp::PubComp;
use crate::packetview::publish::Publish;
use crate::packetview::pubrec::PubRec;
use crate::packetview::pubrel::PubRel;
use crate::packetview::suback::SubAck;
use crate::packetview::subscribe::Subscribe;
use crate::packetview::unsuback::UnsubAck;
use crate::packetview::unsubscribe::Unsubscribe;
use crate::packetview::{Error, FixedHeader, PacketType};

/// Encapsulates all MQTT packet types
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Packet<'a> {
    Connect(Connect<'a>),
    ConnAck(ConnAck),
    Publish(Publish<'a>),
    PubAck(PubAck),
    PubRec(PubRec),
    PubRel(PubRel),
    PubComp(PubComp),
    Subscribe(Subscribe<'a>),
    SubAck(SubAck<'a>),
    Unsubscribe(Unsubscribe<'a>),
    UnsubAck(UnsubAck),
    PingReq,
    PingResp,
    Disconnect,
}

impl<'a> Packet<'a> {
    fn read_exact(fixed_header: FixedHeader, data: &'a [u8]) -> Result<Self, Error> {
        let packet_type = fixed_header.packet_type()?;

        if fixed_header.remaining_len == 0 {
            // no payload packets
            return match packet_type {
                PacketType::PingReq => Ok(Packet::PingReq),
                PacketType::PingResp => Ok(Packet::PingResp),
                PacketType::Disconnect => Ok(Packet::Disconnect),
                _ => Err(Error::MalformedPacket),
            };
        }
        let packet = match packet_type {
            PacketType::Connect => Packet::Connect(Connect::read_exact(fixed_header, data)?),
            PacketType::ConnAck => Packet::ConnAck(ConnAck::read_exact(fixed_header, data)?),
            PacketType::Publish => Packet::Publish(Publish::read_exact(fixed_header, data)?),
            PacketType::PubAck => Packet::PubAck(PubAck::read_exact(fixed_header, data)?),
            PacketType::PubRec => Packet::PubRec(PubRec::read_exact(fixed_header, data)?),
            PacketType::PubRel => Packet::PubRel(PubRel::read_exact(fixed_header, data)?),
            PacketType::PubComp => Packet::PubComp(PubComp::read_exact(fixed_header, data)?),
            PacketType::Subscribe => Packet::Subscribe(Subscribe::read_exact(fixed_header, data)?),
            PacketType::SubAck => Packet::SubAck(SubAck::read_exact(fixed_header, data)?),
            PacketType::Unsubscribe => {
                Packet::Unsubscribe(Unsubscribe::read_exact(fixed_header, data)?)
            }
            PacketType::UnsubAck => Packet::UnsubAck(UnsubAck::read_exact(fixed_header, data)?),
            PacketType::PingReq => Packet::PingReq,
            PacketType::PingResp => Packet::PingResp,
            PacketType::Disconnect => Packet::Disconnect,
        };

        Ok(packet)
    }

    /// Reads a stream of bytes and extracts next MQTT packet out of it
    pub fn read(data: &'a [u8], max_size: usize) -> Result<(Self, usize), Error> {
        let fixed_header = super::check(data, max_size)?;

        // Test with a stream with exactly the size to check border panics
        let packet = &data[0..fixed_header.frame_length()];
        let parsed = Self::read_exact(fixed_header, packet)?;
        Ok((parsed, packet.len()))
    }

    pub fn packet_type(&self) -> PacketType {
        match self {
            Packet::Connect(_) => PacketType::Connect,
            Packet::ConnAck(_) => PacketType::ConnAck,
            Packet::Publish(_) => PacketType::Publish,
            Packet::PubAck(_) => PacketType::PubAck,
            Packet::PubRec(_) => PacketType::PubRec,
            Packet::PubRel(_) => PacketType::PubRel,
            Packet::PubComp(_) => PacketType::PubComp,
            Packet::Subscribe(_) => PacketType::Subscribe,
            Packet::SubAck(_) => PacketType::SubAck,
            Packet::Unsubscribe(_) => PacketType::Unsubscribe,
            Packet::UnsubAck(_) => PacketType::UnsubAck,
            Packet::PingReq => PacketType::PingReq,
            Packet::PingResp => PacketType::PingResp,
            Packet::Disconnect => PacketType::Disconnect,
        }
    }
}

pub struct PacketReadData<'a> {
    pub packet: Packet<'a>,
    pub taken: usize,
}

#[cfg(test)]
mod tests {
    use crate::packetview::packet::Packet;

    #[test]
    fn ping_response() {
        let data = [0xD0, 0, 0xD0, 0];
        let (packet, taken) = Packet::read(&data, 20).unwrap();
        assert_eq!(taken, 2);
        assert_eq!(packet, Packet::PingResp);
    }
}
