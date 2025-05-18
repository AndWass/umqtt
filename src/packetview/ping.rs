use crate::packetview::WriteError;
use crate::packetview::cursor::WriteCursor;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PingReq;

impl PingReq {
    pub fn write(&self, buffer: &mut [u8]) -> Result<usize, WriteError> {
        let mut buffer = WriteCursor::new(buffer);
        buffer.put_slice(&[0xC0, 0x00])?;
        Ok(2)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PingResp;

impl PingResp {
    pub fn write(&self, buffer: &mut [u8]) -> Result<usize, WriteError> {
        let mut buffer = WriteCursor::new(buffer);
        buffer.put_slice(&[0xD0, 0x00])?;
        Ok(2)
    }
}
