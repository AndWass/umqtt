use crate::packetview::cursor::{Cursor, WriteCursor};
use crate::packetview::{read_u16, write_remaining_length, Error, FixedHeader, WriteError};

/// Acknowledgement to QoS2 publish
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PubRec {
    pub pkid: u16,
}

impl PubRec {
    pub fn new(pkid: u16) -> PubRec {
        PubRec { pkid }
    }

    fn len(&self) -> usize {
        // pkid
        2
    }

    pub fn read_exact(fixed_header: FixedHeader, bytes: &[u8]) -> Result<Self, Error> {
        let variable_header_index = fixed_header.fixed_header_len;
        let mut bytes = Cursor(bytes);
        bytes.advance(variable_header_index);
        let pkid = read_u16(&mut bytes)?;

        let puback = PubRec { pkid };

        Ok(puback)
    }

    pub fn write(&self, buffer: &mut WriteCursor) -> Result<(), WriteError> {
        let len = self.len();
        buffer.put_u8(0x50)?;
        write_remaining_length(buffer, len)?;
        buffer.put_u16(self.pkid)?;
        Ok(())
    }
}
