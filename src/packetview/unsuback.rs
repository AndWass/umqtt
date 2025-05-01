use crate::packetview::{read_u16, Error, FixedHeader, WriteError};
use crate::packetview::cursor::{Cursor, WriteCursor};

/// Acknowledgement to unsubscribe
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsubAck {
    pub pkid: u16,
}

impl UnsubAck {
    pub fn new(pkid: u16) -> UnsubAck {
        UnsubAck { pkid }
    }

    pub fn read_exact(fixed_header: FixedHeader, bytes: &[u8]) -> Result<Self, Error> {
        if fixed_header.remaining_len != 2 {
            return Err(Error::MalformedPacket);
        }

        let variable_header_index = fixed_header.fixed_header_len;
        let mut bytes = Cursor(bytes);
        bytes.advance(variable_header_index);
        let pkid = read_u16(&mut bytes)?;
        let unsuback = UnsubAck { pkid };

        Ok(unsuback)
    }

    pub fn write(&self, buffer: &mut [u8]) -> Result<usize, WriteError> {
        let mut buffer = WriteCursor::new(buffer);
        buffer.put_slice(&[0xB0, 0x02])?;
        buffer.put_u16(self.pkid)?;
        Ok(buffer.bytes_written())
    }
}
