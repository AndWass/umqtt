use crate::packetview::cursor::{Cursor, WriteCursor};
use crate::packetview::{
    Error, FixedHeader, Protocol, QoS, WriteError, qos, read_mqtt_bytes, read_mqtt_string, read_u8,
    read_u16, write_mqtt_bytes, write_mqtt_string, write_remaining_length,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Login<'a> {
    pub username: &'a str,
    pub password: &'a [u8],
}

impl<'a> Login<'a> {
    pub fn new(username: &'a str, password: &'a [u8]) -> Self {
        Self { username, password }
    }
    fn read(connect_flags: u8, bytes: &mut Cursor<'a>) -> Result<Option<Login<'a>>, Error> {
        let username = match connect_flags & 0b1000_0000 {
            0 => "",
            _ => read_mqtt_string(bytes)?,
        };

        let password = match connect_flags & 0b0100_0000 {
            0 => &[],
            _ => read_mqtt_bytes(bytes)?,
        };

        if username.is_empty() && password.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Login { username, password }))
        }
    }

    fn len(&self) -> usize {
        let mut len = 0;

        if !self.username.is_empty() {
            len += 2 + self.username.len();
        }

        if !self.password.is_empty() {
            len += 2 + self.password.len();
        }

        len
    }

    pub fn write(&self, buffer: &mut WriteCursor) -> Result<u8, WriteError> {
        let mut connect_flags = 0;
        if !self.username.is_empty() {
            connect_flags |= 0x80;
            write_mqtt_string(buffer, self.username)?;
        }

        if !self.password.is_empty() {
            connect_flags |= 0x40;
            write_mqtt_bytes(buffer, self.password)?;
        }

        Ok(connect_flags)
    }
}

/// LastWill that broker forwards on behalf of the client
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastWill<'a> {
    pub topic: &'a str,
    pub message: &'a [u8],
    pub qos: QoS,
    pub retain: bool,
}

impl<'a> LastWill<'a> {
    pub fn new(topic: &'a str, payload: &'a [u8], qos: QoS, retain: bool) -> Self {
        Self {
            topic,
            message: payload,
            qos,
            retain,
        }
    }

    fn read(connect_flags: u8, bytes: &mut Cursor<'a>) -> Result<Option<LastWill<'a>>, Error> {
        let last_will = match connect_flags & 0b100 {
            0 if (connect_flags & 0b0011_1000) != 0 => {
                return Err(Error::MalformedPacket);
            }
            0 => None,
            _ => {
                let will_topic = read_mqtt_string(bytes)?;
                let will_message = read_mqtt_bytes(bytes)?;
                let will_qos = qos((connect_flags & 0b11000) >> 3)?;
                Some(LastWill {
                    topic: will_topic,
                    message: will_message,
                    qos: will_qos,
                    retain: (connect_flags & 0b0010_0000) != 0,
                })
            }
        };

        Ok(last_will)
    }

    fn len(&self) -> usize {
        let mut len = 0;
        len += 2 + self.topic.len() + 2 + self.message.len();
        len
    }

    fn write(&self, buffer: &mut WriteCursor) -> Result<u8, WriteError> {
        let mut connect_flags = 0;

        connect_flags |= 0x04 | ((self.qos as u8) << 3);
        if self.retain {
            connect_flags |= 0x20;
        }

        write_mqtt_string(buffer, self.topic)?;
        write_mqtt_bytes(buffer, self.message)?;
        Ok(connect_flags)
    }
}

#[derive(Debug, Default)]
pub struct ConnectOptions<'a> {
    pub client_id: &'a str,
    pub last_will: Option<LastWill<'a>>,
    pub login: Option<Login<'a>>,
    pub keep_alive: u16,
    pub clean_session: bool,
}

impl<'a> ConnectOptions<'a> {
    pub fn as_connect(&self) -> Connect<'a> {
        Connect {
            clean_session: self.clean_session,
            keep_alive: self.keep_alive,
            client_id: self.client_id,
            login: self.login.clone(),
            last_will: self.last_will.clone(),
            protocol: Protocol::V4,
        }
    }
}

/// Connection packet initiated by the client
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connect<'a> {
    /// Protocol level
    pub protocol: Protocol,
    /// Mqtt keep alive time
    pub keep_alive: u16,
    /// Client Id
    pub client_id: &'a str,
    /// Clean session. Asks the broker to clear previous state
    pub clean_session: bool,
    /// Will that broker needs to publish when the client disconnects
    pub last_will: Option<LastWill<'a>>,
    /// Login credentials
    pub login: Option<Login<'a>>,
}

impl<'a> Connect<'a> {
    pub fn read_exact(fixed_header: FixedHeader, bytes: &'a [u8]) -> Result<Self, Error> {
        let mut bytes = Cursor(bytes);
        bytes.advance(fixed_header.fixed_header_len);

        // Variable header
        let protocol_name = read_mqtt_string(&mut bytes)?;
        let protocol_level = read_u8(&mut bytes)?;
        if protocol_name != "MQTT" {
            return Err(Error::InvalidProtocol);
        }

        let protocol = match protocol_level {
            4 => Protocol::V4,
            num => return Err(Error::InvalidProtocolLevel(num)),
        };

        let connect_flags = read_u8(&mut bytes)?;
        let clean_session = (connect_flags & 0b10) != 0;
        let keep_alive = read_u16(&mut bytes)?;

        let client_id = read_mqtt_string(&mut bytes)?;
        let last_will = LastWill::read(connect_flags, &mut bytes)?;
        let login = Login::read(connect_flags, &mut bytes)?;

        let connect = Connect {
            protocol,
            keep_alive,
            client_id,
            clean_session,
            last_will,
            login,
        };

        Ok(connect)
    }

    fn len(&self) -> usize {
        let mut len = 2 + "MQTT".len() // protocol name
            + 1            // protocol version
            + 1            // connect flags
            + 2; // keep alive

        len += 2 + self.client_id.len();

        // last will len
        if let Some(last_will) = &self.last_will {
            len += last_will.len();
        }

        // username and password len
        if let Some(login) = &self.login {
            len += login.len();
        }

        len
    }

    pub fn write(&self, buffer: &mut [u8]) -> Result<usize, WriteError> {
        let mut buffer = WriteCursor::new(buffer);
        let len = self.len();
        buffer.put_u8(0b0001_0000)?;
        write_remaining_length(&mut buffer, len)?;
        write_mqtt_string(&mut buffer, "MQTT")?;

        match self.protocol {
            Protocol::V4 => buffer.put_u8(0x04)?,
            Protocol::V5 => buffer.put_u8(0x05)?,
        }

        let mut connect_flags = 0;
        if self.clean_session {
            connect_flags |= 0x02;
        }

        let connect_flags_index = buffer.bytes_written();
        buffer.put_u8(connect_flags)?;
        buffer.put_u16(self.keep_alive)?;
        write_mqtt_string(&mut buffer, self.client_id)?;

        if let Some(last_will) = &self.last_will {
            connect_flags |= last_will.write(&mut buffer)?;
        }

        if let Some(login) = &self.login {
            connect_flags |= login.write(&mut buffer)?;
        }

        buffer[connect_flags_index] = connect_flags;

        Ok(buffer.bytes_written())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::packetview::parse_fixed_header;

    #[test]
    fn connect_parsing_works() {
        let packetstream = [
            0x10,
            39, // packet type, flags and remaining len
            0x00,
            0x04,
            b'M',
            b'Q',
            b'T',
            b'T',
            0x04,        // variable header
            0b1100_1110, // variable header. +username, +password, -will retain, will qos=1, +last_will, +clean_session
            0x00,
            0x0a, // variable header. keep alive = 10 sec
            0x00,
            0x04,
            b't',
            b'e',
            b's',
            b't', // payload. client_id
            0x00,
            0x02,
            b'/',
            b'a', // payload. will topic = '/a'
            0x00,
            0x07,
            b'o',
            b'f',
            b'f',
            b'l',
            b'i',
            b'n',
            b'e', // payload. variable header. will msg = 'offline'
            0x00,
            0x04,
            b'r',
            b'u',
            b'm',
            b'q', // payload. username = 'rumq'
            0x00,
            0x02,
            b'm',
            b'q', // payload. password = 'mq'
            0xDE,
            0xAD,
            0xBE,
            0xEF, // extra packets in the stream
        ];

        let fixed_header = parse_fixed_header(&packetstream).unwrap();
        let connect_bytes = &packetstream[..fixed_header.frame_length()];
        let packet = Connect::read_exact(fixed_header, connect_bytes).unwrap();

        assert_eq!(
            packet,
            Connect {
                protocol: Protocol::V4,
                keep_alive: 10,
                client_id: "test",
                clean_session: true,
                last_will: Some(LastWill::new("/a", b"offline", QoS::AtLeastOnce, false)),
                login: Some(Login::new("rumq", b"mq")),
            }
        );
    }

    fn sample_bytes() -> &'static [u8] {
        &[
            0x10,
            39,
            0x00,
            0x04,
            b'M',
            b'Q',
            b'T',
            b'T',
            0x04,
            0b1100_1110, // +username, +password, -will retain, will qos=1, +last_will, +clean_session
            0x00,
            0x0a, // 10 sec
            0x00,
            0x04,
            b't',
            b'e',
            b's',
            b't', // client_id
            0x00,
            0x02,
            b'/',
            b'a', // will topic = '/a'
            0x00,
            0x07,
            b'o',
            b'f',
            b'f',
            b'l',
            b'i',
            b'n',
            b'e', // will msg = 'offline'
            0x00,
            0x04,
            b'r',
            b'u',
            b's',
            b't', // username = 'rust'
            0x00,
            0x02,
            b'm',
            b'q', // password = 'mq'
        ]
    }

    #[test]
    fn connect_encoding_works() {
        let connect = Connect {
            protocol: Protocol::V4,
            keep_alive: 10,
            client_id: "test",
            clean_session: true,
            last_will: Some(LastWill::new("/a", b"offline", QoS::AtLeastOnce, false)),
            login: Some(Login::new("rust", b"mq")),
        };

        let mut buffer = [0u8; 256];
        let written = connect.write(&mut buffer).unwrap();

        assert_eq!(&buffer[..written], sample_bytes());
    }
}
