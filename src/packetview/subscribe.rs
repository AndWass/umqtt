use crate::packetview::borrowed_buf::BorrowedBuf;
use crate::packetview::cursor::{Cursor, WriteCursor};
use crate::packetview::{
    Error, FixedHeader, QoS, WriteError, qos, read_u16, write_mqtt_string, write_remaining_length,
};

pub struct BytesIterator<'a>(core::slice::Iter<'a, u8>);

impl<'a> Iterator for BytesIterator<'a> {
    type Item = SubscribeFilter<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut iter = self.0.clone();
        let topic_len = [*iter.next()?, *iter.next()?];
        let topic_len = u16::from_be_bytes(topic_len) as usize;
        let data_slice = iter.as_slice();
        if data_slice.len() < 1 + topic_len {
            return None;
        }

        let path = core::str::from_utf8(&data_slice[0..topic_len]).ok()?;
        let qos = qos(data_slice[topic_len]).ok()?;

        self.0 = data_slice[topic_len + 1..].iter();
        Some(SubscribeFilter { qos, path })
    }
}

#[derive(Clone, Eq)]
pub enum Storage<'a> {
    Slice(&'a [SubscribeFilter<'a>]),
    WireBytes(&'a [u8]),
}

impl<'a> Storage<'a> {
    fn iter(&self) -> StorageIter<'a> {
        match self {
            Storage::Slice(slice) => StorageIter::Slice(slice.iter()),
            Storage::WireBytes(bytes) => StorageIter::Bytes(BytesIterator(bytes.iter())),
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Storage::Slice(x) => x.is_empty(),
            Storage::WireBytes(_) => self.iter().count() == 0,
        }
    }
}

impl core::fmt::Debug for Storage<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for x in self.iter() {
            writeln!(f, "{{{:?}}}", x)?;
        }
        Ok(())
    }
}

impl PartialEq for Storage<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter())
    }
}

pub enum StorageIter<'a> {
    Slice(core::slice::Iter<'a, SubscribeFilter<'a>>),
    Bytes(BytesIterator<'a>),
}

impl<'a> Iterator for StorageIter<'a> {
    type Item = SubscribeFilter<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            StorageIter::Slice(iter) => iter.next().cloned(),
            StorageIter::Bytes(iter) => iter.next(),
        }
    }
}

/// Subscription packet
#[derive(Clone, PartialEq, Eq)]
pub struct Subscribe<'a> {
    pub pkid: u16,
    pub filters: Storage<'a>,
}

impl<'a> Subscribe<'a> {
    pub fn new(packet_id: u16, filters: &'a [SubscribeFilter<'a>]) -> Self {
        Self {
            pkid: packet_id,
            filters: Storage::Slice(filters),
        }
    }

    fn len(&self) -> usize {
        // len of pkid + subscribe filter len
        let filter_len = match &self.filters {
            Storage::Slice(slice) => slice.iter().fold(0, |s, t| s + t.len()),
            Storage::WireBytes(bytes) => BytesIterator(bytes.iter()).fold(0, |s, t| s + t.len()),
        };
        2 + filter_len
    }

    pub fn read_exact(fixed_header: FixedHeader, bytes: &'a [u8]) -> Result<Self, Error> {
        let variable_header_index = fixed_header.fixed_header_len;
        let mut bytes = Cursor(bytes);
        bytes.advance(variable_header_index);

        let pkid = read_u16(&mut bytes)?;

        let mut bytes_iter = BytesIterator(bytes.as_slice().iter());

        let filter_count = (&mut bytes_iter).count();

        if filter_count == 0 {
            Err(Error::EmptySubscription)
        } else if !bytes_iter.0.as_slice().is_empty() {
            Err(Error::MalformedPacket)
        } else {
            Ok(Self {
                pkid,
                filters: Storage::WireBytes(bytes.as_slice()),
            })
        }
    }

    pub fn write(&self, buffer: &mut [u8]) -> Result<usize, WriteError> {
        let mut buffer = WriteCursor::new(buffer);
        // write packet type
        buffer.put_u8(0x82)?;

        // write remaining length
        let remaining_len = self.len();
        write_remaining_length(&mut buffer, remaining_len)?;

        // write packet id
        buffer.put_u16(self.pkid)?;

        // write filters
        for filter in self.filters.iter() {
            filter.write(&mut buffer)?;
        }

        Ok(buffer.bytes_written())
    }
}

pub struct SubscribeWriter<'a, 'b>(pub(crate) &'a mut BorrowedBuf<'b>);

impl SubscribeWriter<'_, '_> {
    pub fn add_str(&mut self, topic: &str, qos: QoS) -> Result<(), WriteError> {
        let len = topic.len();
        if len > (u16::MAX as usize) {
            return Err(WriteError::PayloadTooLong);
        }
        self.0.add_slice(&((len as u16).to_be_bytes()))?;
        self.0.add_slice(topic.as_bytes())?;
        self.0.add_slice(&[qos as u8])
    }

    pub fn add_separated(
        &mut self,
        parts: &[&str],
        separator: &str,
        qos: QoS,
    ) -> Result<(), WriteError> {
        let start_len = self.0.len();
        self.0.add_slice(&[0, 0])?; // To be filled in later
        let parts_len = parts.len();
        for (i, part) in parts.iter().enumerate() {
            self.0.add_slice(part.as_bytes())?;
            if i != parts_len - 1 {
                self.0.add_slice(separator.as_bytes())?;
            }
        }
        let len = self.0.len() - start_len - 2;
        if len > (u16::MAX as usize) {
            return Err(WriteError::PayloadTooLong);
        }
        let len = (len as u16).to_be_bytes();
        self.0[start_len] = len[0];
        self.0[start_len+1] = len[1];
        self.0.add_slice(&[qos as u8])
    }
}

///  Subscription filter
#[derive(Clone, PartialEq, Eq)]
pub struct SubscribeFilter<'a> {
    pub path: &'a str,
    pub qos: QoS,
}

impl<'a> SubscribeFilter<'a> {
    pub const fn new(path: &'a str, qos: QoS) -> Self {
        SubscribeFilter { path, qos }
    }

    fn len(&self) -> usize {
        // filter len + filter + options
        2 + self.path.len() + 1
    }

    fn write(&self, buffer: &mut WriteCursor) -> Result<(), WriteError> {
        let mut options = 0;
        options |= self.qos as u8;

        write_mqtt_string(buffer, self.path)?;
        buffer.put_u8(options)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetainForwardRule {
    OnEverySubscribe,
    OnNewSubscribe,
    Never,
}

impl core::fmt::Debug for Subscribe<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "Filters = [{:?}], Packet id = {:?}",
            self.filters, self.pkid
        )
    }
}

impl core::fmt::Debug for SubscribeFilter<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Filter = {}, Qos = {:?}", self.path, self.qos)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::packetview::parse_fixed_header;

    #[test]
    fn subscribe_parsing_works() {
        let stream = [
            0b1000_0010,
            20, // packet type, flags and remaining len
            0x01,
            0x04, // variable header. pkid = 260
            0x00,
            0x03,
            b'a',
            b'/',
            b'+', // payload. topic filter = 'a/+'
            0x00, // payload. qos = 0
            0x00,
            0x01,
            b'#', // payload. topic filter = '#'
            0x01, // payload. qos = 1
            0x00,
            0x05,
            b'a',
            b'/',
            b'b',
            b'/',
            b'c', // payload. topic filter = 'a/b/c'
            0x02, // payload. qos = 2
            0xDE,
            0xAD,
            0xBE,
            0xEF, // extra packets in the stream
        ];
        let fixed_header = parse_fixed_header(&stream).unwrap();
        let subscribe_bytes = &stream[..fixed_header.frame_length()];
        let packet = Subscribe::read_exact(fixed_header, subscribe_bytes).unwrap();

        assert_eq!(
            packet,
            Subscribe {
                pkid: 260,
                filters: Storage::Slice(&[
                    SubscribeFilter::new("a/+", QoS::AtMostOnce),
                    SubscribeFilter::new("#", QoS::AtLeastOnce),
                    SubscribeFilter::new("a/b/c", QoS::ExactlyOnce)
                ]),
            }
        );
    }

    #[test]
    fn subscribe_encoding_works() {
        let filters = [
            SubscribeFilter::new("a/+", QoS::AtMostOnce),
            SubscribeFilter::new("#", QoS::AtLeastOnce),
            SubscribeFilter::new("a/b/c", QoS::ExactlyOnce),
        ];
        let subscribe = Subscribe {
            pkid: 260,
            filters: Storage::Slice(&filters),
        };

        let mut buf = [0u8; 256];
        let written = subscribe.write(&mut buf).unwrap();
        assert_eq!(
            &buf[..written],
            &[
                0b1000_0010,
                20,
                0x01,
                0x04, // pkid = 260
                0x00,
                0x03,
                b'a',
                b'/',
                b'+', // topic filter = 'a/+'
                0x00, // qos = 0
                0x00,
                0x01,
                b'#', // topic filter = '#'
                0x01, // qos = 1
                0x00,
                0x05,
                b'a',
                b'/',
                b'b',
                b'/',
                b'c', // topic filter = 'a/b/c'
                0x02  // qos = 2
            ]
        );
    }

    #[test]
    fn subscribe_writer() {
        {
            let mut buffer = [0; 256];
            let mut borrowed = BorrowedBuf::new(&mut buffer);
            let mut writer = SubscribeWriter(&mut borrowed);
            writer.add_str("hello", QoS::AtMostOnce).unwrap();
            assert_eq!(&writer.0[0..writer.0.len()], b"\x00\x05hello\x00");
        }
        {
            let mut buffer = [0; 256];
            let mut borrowed = BorrowedBuf::new(&mut buffer);
            let mut writer = SubscribeWriter(&mut borrowed);
            writer
                .add_separated(&["hello"], "/", QoS::AtMostOnce)
                .unwrap();
            assert_eq!(&writer.0[0..writer.0.len()], b"\x00\x05hello\x00");
        }
        {
            let mut buffer = [0; 256];
            let mut borrowed = BorrowedBuf::new(&mut buffer);
            let mut writer = SubscribeWriter(&mut borrowed);
            writer
                .add_separated(&["hello", "world"], "/", QoS::AtMostOnce)
                .unwrap();
            assert_eq!(&writer.0[0..writer.0.len()], b"\x00\x0bhello/world\x00");
        }

        {
            let mut buffer = [0; 256];
            let mut borrowed = BorrowedBuf::new(&mut buffer);
            let mut writer = SubscribeWriter(&mut borrowed);
            writer
                .add_separated(&["hello", "world"], "/", QoS::AtMostOnce)
                .unwrap();
            assert_eq!(&writer.0[0..writer.0.len()], b"\x00\x0bhello/world\x00");
            writer.add_separated(&["ab", "cd"], "/", QoS::AtMostOnce).unwrap();
            assert_eq!(&writer.0[0..writer.0.len()], b"\x00\x0bhello/world\x00\x00\x05ab/cd\x00");
        }
    }
}
