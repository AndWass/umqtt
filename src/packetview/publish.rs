use crate::packetview::{qos, read_mqtt_string, read_u16, write_mqtt_string, write_remaining_length, Error, FixedHeader, QoS, WriteError};
use crate::packetview::cursor::{Cursor, WriteCursor};
use crate::packetview::topic_iterator::TopicIterator;

/// Represent a [`Publish`] but where the topic is constructed of multiple parts
///
/// These parts are represented by a [`TopicIterator`].
#[derive(Clone)]
pub struct PublishTopicIter<'a, I: Iterator<Item=&'a str> + Sized + Clone> {
    pub dup: bool,
    pub qos: QoS,
    pub retain: bool,
    pub topic: TopicIterator<'a, I>,
    pub pkid: u16,
    pub payload: &'a [u8],
}

impl<'a, I: Iterator<Item=&'a str> + Clone> PublishTopicIter<'a, I> {
    fn topic_len(&self) -> usize {
        self.topic.clone().map(|x| x.len()).sum()
    }
    fn len(&self) -> usize {
        let len = 2 + self.topic_len() + self.payload.len();
        if self.qos != QoS::AtMostOnce && self.pkid != 0 {
            len + 2
        } else {
            len
        }
    }

    pub fn write(&self, buffer: &mut [u8]) -> Result<usize, WriteError> {
        let mut buffer = WriteCursor::new(buffer);
        let len = self.len();

        let dup = self.dup as u8;
        let qos = self.qos as u8;
        let retain = self.retain as u8;
        buffer.put_u8(0b0011_0000 | retain | (qos << 1) | (dup << 3))?;

        write_remaining_length(&mut buffer, len)?;
        buffer.put_u16(self.topic_len() as u16)?;

        for part in self.topic.clone() {
            buffer.put_slice(part.as_bytes())?;
        }

        if self.qos != QoS::AtMostOnce {
            let pkid = self.pkid;
            if pkid == 0 {
                return Err(WriteError::MalformedPacket);
            }

            buffer.put_u16(pkid)?;
        }

        buffer.put_slice(self.payload)?;

        Ok(buffer.bytes_written())
    }
}

impl<'a> From<Publish<'a>> for PublishTopicIter<'a, core::iter::Once<&'a str>> {
    fn from(publish: Publish<'a>) -> Self {
        Self {
            topic: TopicIterator::new(core::iter::once(publish.topic)),
            dup: publish.dup,
            retain: publish.retain,
            qos: publish.qos,
            pkid: publish.pkid,
            payload: publish.payload,
        }
    }
}

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
        #[derive(Debug)]
        #[allow(dead_code)]
        enum MaybeString<'a> {
            String(&'a str),
            Bytes(&'a [u8]),
        }

        let payload = if let Ok(s) = core::str::from_utf8(self.payload) {
            MaybeString::String(s)
        }
        else {
            MaybeString::Bytes(self.payload)
        };

        write!(
            f,
            "Topic = {}, Qos = {:?}, Retain = {}, Pkid = {:?}, Payload = {:?}",
            self.topic,
            self.qos,
            self.retain,
            self.pkid,
            payload
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

    fn len(&self) -> usize {
        let len = 2 + self.topic.len() + self.payload.len();
        if self.qos != QoS::AtMostOnce && self.pkid != 0 {
            len + 2
        } else {
            len
        }
    }

    pub fn write(&self, buffer: &mut [u8]) -> Result<usize, WriteError> {
        let mut buffer = WriteCursor::new(buffer);
        let len = self.len();

        let dup = self.dup as u8;
        let qos = self.qos as u8;
        let retain = self.retain as u8;
        buffer.put_u8(0b0011_0000 | retain | (qos << 1) | (dup << 3))?;

        write_remaining_length(&mut buffer, len)?;
        write_mqtt_string(&mut buffer, self.topic)?;

        if self.qos != QoS::AtMostOnce {
            let pkid = self.pkid;
            if pkid == 0 {
                return Err(WriteError::MalformedPacket);
            }

            buffer.put_u16(pkid)?;
        }

        buffer.put_slice(self.payload)?;

        Ok(buffer.bytes_written())
    }
}