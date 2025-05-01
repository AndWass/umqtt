use super::*;

/// Acknowledgement to QoS1 publish
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PubAck {
    pub pkid: u16,
}

impl PubAck {
    pub fn new(pkid: u16) -> PubAck {
        PubAck { pkid }
    }

    pub fn read_exact(fixed_header: FixedHeader, bytes: &[u8]) -> Result<Self, Error> {
        let variable_header_index = fixed_header.fixed_header_len;
        let mut bytes = Cursor(bytes);
        bytes.advance(variable_header_index);
        let pkid = read_u16(&mut bytes)?;

        // No reason code or properties if remaining length == 2
        if fixed_header.remaining_len == 2 {
            return Ok(PubAck { pkid });
        }

        // No properties len or properties if remaining len > 2 but < 4
        if fixed_header.remaining_len < 4 {
            return Ok(PubAck { pkid });
        }

        let puback = PubAck { pkid };

        Ok(puback)
    }

    pub fn write(&self, buffer: &mut [u8]) -> Result<usize, WriteError> {
        let mut buffer = WriteCursor::new(buffer);
        buffer.put_u8(0x40)?;
        write_remaining_length(&mut buffer, 2)?;
        buffer.put_u16(self.pkid)?;
        Ok(buffer.bytes_written())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn puback_encoding_works() {
        let stream = [
            0b0100_0000,
            0x02, // packet type, flags and remaining len
            0x00,
            0x0A, // fixed header. packet identifier = 10
            0xDE,
            0xAD,
            0xBE,
            0xEF, // extra packets in the stream
        ];
        let fixed_header = parse_fixed_header(&stream).unwrap();
        let ack_bytes = &stream[0..fixed_header.frame_length()];
        let packet = PubAck::read_exact(fixed_header, ack_bytes).unwrap();

        assert_eq!(packet, PubAck { pkid: 10 });
    }
}
