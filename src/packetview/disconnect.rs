use crate::packetview::WriteError;
use crate::packetview::cursor::WriteCursor;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disconnect;

impl Disconnect {
    pub fn write(&self, buffer: &mut [u8]) -> Result<usize, WriteError> {
        let mut buffer = WriteCursor::new(buffer);
        buffer.put_slice(&[0xE0, 0x00])?;
        Ok(2)
    }
}
