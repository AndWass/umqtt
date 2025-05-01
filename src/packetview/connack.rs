use crate::packetview::{read_u8, write_remaining_length, Error, FixedHeader, WriteError};
use crate::packetview::cursor::{Cursor, WriteCursor};

/// Return code in connack
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ConnectReturnCode {
    Success = 0,
    RefusedProtocolVersion,
    BadClientId,
    ServiceUnavailable,
    BadUserNamePassword,
    NotAuthorized,
}

/// Connection return code type
fn connect_return(num: u8) -> Result<ConnectReturnCode, Error> {
    match num {
        0 => Ok(ConnectReturnCode::Success),
        1 => Ok(ConnectReturnCode::RefusedProtocolVersion),
        2 => Ok(ConnectReturnCode::BadClientId),
        3 => Ok(ConnectReturnCode::ServiceUnavailable),
        4 => Ok(ConnectReturnCode::BadUserNamePassword),
        5 => Ok(ConnectReturnCode::NotAuthorized),
        num => Err(Error::InvalidConnectReturnCode(num)),
    }
}

/// Acknowledgement to connect packet
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnAck {
    pub session_present: bool,
    pub code: ConnectReturnCode,
}

impl ConnAck {

    pub const fn new(session_present: bool, code: ConnectReturnCode) -> Self {
        Self { session_present, code }
    }

    fn len(&self) -> usize {
        // sesssion present + code

        1 + 1
    }

    pub fn read_exact(fixed_header: FixedHeader, bytes: &[u8]) -> Result<Self, Error> {
        let variable_header_index = fixed_header.fixed_header_len;
        let mut bytes = Cursor(bytes);
        bytes.advance(variable_header_index);

        let flags = read_u8(&mut bytes)?;
        let return_code = read_u8(&mut bytes)?;

        let session_present = (flags & 0x01) == 1;
        let code = connect_return(return_code)?;
        let connack = ConnAck {
            session_present,
            code,
        };

        Ok(connack)
    }

    pub fn write(&self, buffer: &mut [u8]) -> Result<usize, WriteError> {
        let mut buffer = WriteCursor::new(buffer);
        let len = self.len();
        buffer.put_u8(0x20)?;

        write_remaining_length(&mut buffer, len)?;
        buffer.put_u8(self.session_present as u8)?;
        buffer.put_u8(self.code as u8)?;

        Ok(buffer.bytes_written())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packetview::cursor::WriteCursor;
    use crate::packetview::parse_fixed_header;

    #[test]
    fn connack_parsing_works() {
        let data = [
            0b0010_0000,
            0x02, // packet type, flags and remaining len
            0x01,
            0x00, // variable header. connack flags, connect return code
            0xDE,
            0xAD,
            0xBE,
            0xEF, // extra packets in the stream
        ];

        let fixed_header = parse_fixed_header(&data).unwrap();
        let connack_bytes = &data[..fixed_header.frame_length()];
        let connack = ConnAck::read_exact(fixed_header, connack_bytes).unwrap();

        assert_eq!(
            connack,
            ConnAck {
                session_present: true,
                code: ConnectReturnCode::Success,
            }
        );
    }

    #[test]
    fn connack_encoding_works() {
        let connack = ConnAck {
            session_present: true,
            code: ConnectReturnCode::Success,
        };

        let mut buffer = [0u8; 256];
        let bytes_written = connack.write(&mut buffer).unwrap();
        assert_eq!(&buffer[..bytes_written], [0b0010_0000, 0x02, 0x01, 0x00]);
    }
}