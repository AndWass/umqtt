use core::fmt::Debug;
use super::*;

pub struct WireBytesIter<'a>(core::slice::Iter<'a, u8>);

impl<'a> Iterator for WireBytesIter<'a> {
    type Item = &'a str;
    fn next(&mut self) -> Option<Self::Item> {
        let slice = self.0.as_slice();
        let mut cursor = Cursor(slice);
        let res = read_mqtt_string(&mut cursor).ok()?;
        self.0 = slice[2+res.len()..].iter();
        Some(res)
    }
}

pub enum StorageIterator<'a> {
    Slice(core::slice::Iter<'a, &'a str>),
    WireBytes(WireBytesIter<'a>)
}

impl<'a> Iterator for StorageIterator<'a> {
    type Item = &'a str;
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            StorageIterator::Slice(s) => s.next().cloned(),
            StorageIterator::WireBytes(b) => b.next(),
        }
    }
}

#[derive(Eq, Clone)]
pub enum Storage<'a> {
    Slice(&'a [&'a str]),
    WireBytes(&'a [u8]),
}

impl<'a> Storage<'a> {
    pub fn iter(&self) -> StorageIterator<'a> {
        match self {
            Storage::Slice(s) => StorageIterator::Slice((*s).iter()),
            Storage::WireBytes(b) => StorageIterator::WireBytes(WireBytesIter(b.iter())),
        }
    }
}

impl Debug for Storage<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for t in self.iter() {
            write!(f, "{:?}", t)?;
        }

        Ok(())
    }
}

impl PartialEq for Storage<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter())
    }
}

/// Unsubscribe packet
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsubscribe<'a> {
    pub pkid: u16,
    pub topics: Storage<'a>,
}

impl<'a> Unsubscribe<'a> {
    fn len(&self) -> usize {
        // len of pkid + vec![subscribe topics len]
        2 + self.topics.iter().fold(0, |s, t| s + t.len() + 2)
    }

    pub fn read_exact(fixed_header: FixedHeader, bytes: &'a [u8]) -> Result<Self, Error> {
        let variable_header_index = fixed_header.fixed_header_len;
        let mut bytes = Cursor(bytes);
        bytes.advance(variable_header_index);

        let pkid = read_u16(&mut bytes)?;

        let topics = bytes.as_slice();

        while !bytes.as_slice().is_empty() {
            read_mqtt_string(&mut bytes)?;
        }

        let unsubscribe = Unsubscribe { pkid, topics: Storage::WireBytes(topics) };
        Ok(unsubscribe)
    }

    pub fn write(&self, payload: &mut WriteCursor) -> Result<(), WriteError> {
        let remaining_len = self.len();

        payload.put_u8(0xA2)?;
        write_remaining_length(payload, remaining_len)?;
        payload.put_u16(self.pkid)?;

        for topic in self.topics.iter() {
            write_mqtt_string(payload, topic)?;
        }
        Ok(())
    }
}
