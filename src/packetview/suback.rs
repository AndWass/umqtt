use core::convert::{TryFrom, TryInto};
use crate::packetview::{read_u16, write_remaining_length, Error, FixedHeader, QoS, WriteError};
use crate::packetview::cursor::{Cursor, WriteCursor};

pub struct BytesIterator<'a>(core::slice::Iter<'a, u8>);

impl<'a> Iterator for BytesIterator<'a> {
    type Item = SubscribeReasonCode;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().copied()?.try_into().ok()
    }
}

pub enum StorageIter<'a> {
    Slice(core::slice::Iter<'a, SubscribeReasonCode>),
    Bytes(BytesIterator<'a>),
}

impl<'a> Iterator for StorageIter<'a> {
    type Item = SubscribeReasonCode;
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            StorageIter::Slice(iter) => iter.next().copied(),
            StorageIter::Bytes(iter) => iter.next(),
        }
    }
}

#[derive(Clone, Eq)]
pub enum Storage<'a> {
    Slice(&'a [SubscribeReasonCode]),
    Bytes(&'a [u8])
}

impl<'a> Storage<'a> {
    pub fn iter(&self) -> StorageIter<'a> {
        match self {
            Storage::Slice(s) => StorageIter::Slice(s.iter()),
            Storage::Bytes(b) => StorageIter::Bytes(BytesIterator(b.iter())),
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Storage::Slice(s) => s.len(),
            Storage::Bytes(b) => b.len(),
        }
    }
}

impl<'a> core::fmt::Debug for Storage<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for c in self.iter() {
            write!(f, "{:?}\n", c)?;
        }

        Ok(())
    }
}

impl<'a> PartialEq for Storage<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter())
    }
}

/// Acknowledgement to subscribe
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubAck<'a> {
    pub pkid: u16,
    pub return_codes: Storage<'a>,
}

impl<'a> SubAck<'a> {
    pub fn new(pkid: u16, return_codes: &'a [SubscribeReasonCode]) -> Self {
        SubAck { pkid, return_codes: Storage::Slice(return_codes) }
    }

    fn len(&self) -> usize {
        2 + self.return_codes.len()
    }

    pub fn read_exact(fixed_header: FixedHeader, bytes: &'a [u8]) -> Result<Self, Error> {
        let variable_header_index = fixed_header.fixed_header_len;
        let mut bytes = Cursor(bytes);
        bytes.advance(variable_header_index);
        let pkid = read_u16(&mut bytes)?;

        if bytes.as_slice().is_empty() {
            return Err(Error::MalformedPacket);
        }

        for byte in bytes.as_slice() {
            let _: SubscribeReasonCode = (*byte).try_into()?;
        }

        let suback = SubAck { pkid, return_codes: Storage::Bytes(bytes.as_slice()) };
        Ok(suback)
    }

    pub fn write(&self, buffer: &mut WriteCursor) -> Result<(), WriteError> {
        buffer.put_u8(0x90)?;
        let remaining_len = self.len();
        write_remaining_length(buffer, remaining_len)?;

        buffer.put_u16(self.pkid)?;
        for code in self.return_codes.iter() {
            let byte = match code {
                SubscribeReasonCode::Success(qos) => qos as u8,
                SubscribeReasonCode::Failure => 0x80,
            };
            buffer.put_u8(byte)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscribeReasonCode {
    Success(QoS),
    Failure,
}

impl TryFrom<u8> for SubscribeReasonCode {
    type Error = super::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        let v = match value {
            0 => SubscribeReasonCode::Success(QoS::AtMostOnce),
            1 => SubscribeReasonCode::Success(QoS::AtLeastOnce),
            2 => SubscribeReasonCode::Success(QoS::ExactlyOnce),
            128 => SubscribeReasonCode::Failure,
            v => return Err(Error::InvalidSubscribeReasonCode(v)),
        };

        Ok(v)
    }
}

#[cfg(test)]
mod test {
    use crate::packetview::parse_fixed_header;
    use super::*;

    #[test]
    fn suback_parsing_works() {
        let stream = [
            0x90, 4, // packet type, flags and remaining len
            0x00, 0x0F, // variable header. pkid = 15
            0x01, 0x80, // payload. return codes [success qos1, failure]
            0xDE, 0xAD, 0xBE, 0xEF, // extra packets in the stream
        ];

        let fixed_header = parse_fixed_header(&stream).unwrap();
        let ack_bytes = &stream[..fixed_header.frame_length()];
        let packet = SubAck::read_exact(fixed_header, ack_bytes).unwrap();

        assert_eq!(
            packet,
            SubAck {
                pkid: 15,
                return_codes: Storage::Slice(&[
                    SubscribeReasonCode::Success(QoS::AtLeastOnce),
                    SubscribeReasonCode::Failure,
                ]),
            }
        );
    }
}
