use crate::packetview::{qos, read_mqtt_string, read_u16, write_mqtt_string, write_remaining_length, Error, FixedHeader, QoS, WriteError};
use crate::packetview::borrowed_buf::BorrowedBuf;
use crate::packetview::cursor::{Cursor, WriteCursor};

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

/// A type that represents a writable publish with lazy topic and payload formatting.
///
/// Unlike the [`Publish`] struct this allows for forming topic and payload
/// in place in the buffer directly. This way one doesn't have to store the complete
/// topic or payload in an intermediate buffer before copying it to the output buffer;
/// the topic and payload can be written directly to the output buffer.
#[derive(Copy, Clone, Debug)]
pub struct OutPublish {
    pub dup: bool,
    pub qos: QoS,
    pub retain: bool,
    pub pkid: u16,
}

/// The argument to the `FnOnce` when writing a publish using [`OutPublish`]
pub struct TopicWriter<'a, 'b: 'a>(&'a mut BorrowedBuf<'b>);

impl<'a, 'b: 'a> TopicWriter<'a, 'b> {

    pub fn capacity(&self) -> usize {
        self.0.capacity()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_full(&self) -> bool {
        self.len() == self.capacity()
    }

    pub fn add_str(&mut self, string: &str) -> Result<(), WriteError> {
        self.0.add_slice(string.as_bytes())
    }
}

impl<'a, 'b: 'a> core::fmt::Write for TopicWriter<'a, 'b> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.add_str(s).map_err(|_| core::fmt::Error)
    }
}

/// The argument to the `FnOnce` when writing a publish using [`OutPublish`]
pub struct PayloadWriter<'a, 'b: 'a>(&'a mut BorrowedBuf<'b>);

impl<'a, 'b: 'a> PayloadWriter<'a, 'b> {

    pub fn capacity(&self) -> usize {
        self.0.capacity()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_full(&self) -> bool {
        self.len() == self.capacity()
    }

    pub fn add_u8(&mut self, byte: u8) -> Result<(), WriteError> {
        self.add_slice(&[byte])
    }

    pub fn add_slice(&mut self, slice: &[u8]) -> Result<(), WriteError> {
        self.0.add_slice(slice)
    }

    pub fn add_str(&mut self, string: &str) -> Result<(), WriteError> {
        self.add_slice(string.as_bytes())
    }
}

impl<'a, 'b: 'a> core::fmt::Write for PayloadWriter<'a, 'b> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.add_str(s).map_err(|_| core::fmt::Error)
    }
}

impl OutPublish {
    fn write_topic<T: FnOnce(&mut TopicWriter) -> Result<(), WriteError>>(topic: T, buffer: &mut BorrowedBuf) -> Result<(), WriteError> {
        let topic_len_start_index = buffer.len();
        buffer.add_slice(&[0, 0])?;
        let len_before_topic_string = buffer.len();
        topic(&mut TopicWriter(buffer))?;
        let topic_len = buffer.len() - len_before_topic_string;
        if topic_len > (u16::MAX as usize) {
            return Err(WriteError::PayloadTooLong);
        }

        let len_bytes = (topic_len as u16).to_be_bytes();
        (&mut buffer[topic_len_start_index..topic_len_start_index+2]).copy_from_slice(&len_bytes);

        Ok(())
    }
    fn write_var_header_and_payload<T, P>(&self, topic: T, payload: P, output_buffer: &mut [u8]) -> Result<usize, WriteError>
    where
        T: FnOnce(&mut TopicWriter) -> Result<(), WriteError>,
        P: FnOnce(&mut PayloadWriter) -> Result<(), WriteError>,
    {
        let mut borrowed_buf = BorrowedBuf::new(output_buffer);
        Self::write_topic(topic, &mut borrowed_buf)?;
        if self.qos != QoS::AtMostOnce {
            let pkid = self.pkid;
            if pkid == 0 {
                return Err(WriteError::MalformedPacket);
            }

            borrowed_buf.add_slice(&(pkid.to_be_bytes()))?;
        }

        payload(&mut PayloadWriter(&mut borrowed_buf))?;

        Ok(borrowed_buf.len())
    }
    pub fn write<'a, T, P>(&self, topic: T, payload: P, output_buffer: &'a mut [u8]) -> Result<&'a [u8], WriteError>
    where
        T: FnOnce(&mut TopicWriter) -> Result<(), WriteError>,
        P: FnOnce(&mut PayloadWriter) -> Result<(), WriteError>,
    {
        // The fixed header is maximum 5 bytes, so reserve space for this
        // so we can write the fixed header after the variable header and payload
        // is written.
        let (header, remaining) = output_buffer.split_at_mut(5);

        let remaining_len_value = self.write_var_header_and_payload(topic, payload, remaining)?;

        let mut remaining_len_buffer = [0u8; 4];
        let mut remaining_len_buffer = WriteCursor::new(&mut remaining_len_buffer);
        write_remaining_length(&mut remaining_len_buffer, remaining_len_value)?;
        let (remaining_len, _)  = remaining_len_buffer.finish();

        let packet_start_index = 4-remaining_len.len();
        let dup = self.dup as u8;
        let qos = self.qos as u8;
        let retain = self.retain as u8;

        header[packet_start_index] = 0b0011_0000 | retain | (qos << 1) | (dup << 3);
        (&mut header[packet_start_index+1..]).copy_from_slice(remaining_len);

        Ok(&output_buffer[packet_start_index..(1+packet_start_index+remaining_len.len()+remaining_len_value)])
    }
}

#[cfg(test)]
mod tests
{
    use super::*;
    #[test]
    fn out_publish_qos0() {
        let mut buffer = [0;256];
        let output = OutPublish {
            dup: false,
            pkid: 0,
            qos: QoS::AtMostOnce,
            retain: false,
        }.write(|x| x.add_str("hello"), |x| x.add_str("world"), &mut buffer).unwrap();
        assert_eq!(output, b"\x30\x0c\x00\x05helloworld");
    }

    #[test]
    fn out_publish_multibyte_remaining_len_qos0() {
        let mut buffer = [0;256];
        let output = OutPublish {
            dup: false,
            pkid: 0,
            qos: QoS::AtMostOnce,
            retain: false,
        }.write(|x| x.add_str("hello"), |x| {
            core::iter::repeat(1).take(128).enumerate().for_each(|(i, _)| x.add_u8(i as u8).unwrap());
            Ok(())
        }, &mut buffer).unwrap();

        assert_eq!(output.len(), 1+2+2+5+128);
        assert_eq!(&output[0..10], b"\x30\x87\x01\x00\x05hello");
        assert!((&output[10..]).iter().enumerate().all(|x| x.0 == (*x.1).into()));
    }
}
