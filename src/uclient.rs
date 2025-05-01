use core::fmt::{Debug, Formatter};
use crate::packetview::cursor::WriteCursor;
use crate::packetview::publish::Publish;
use crate::packetview::QoS;
use crate::packetview::subscribe::{Subscribe, SubscribeFilter};
use crate::transport_client::TransportClient;

pub enum Error<T: Platform> {
    IO(T::Error),
    WriteError(crate::packetview::WriteError)
}

impl<T: Platform> Debug for Error<T>
where
    T::Error: Debug {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::IO(e) => write!(f, "IO({:?})", e),
            Error::WriteError(e) => write!(f, "WriteError({:?})", e)
        }
    }
}

impl<T: Platform> From<T::Error> for Error<T> {
    fn from(value: T::Error) -> Self {
        Self::IO(value)
    }
}

pub trait Platform {
    type Error;
    async fn connect_transport(&mut self) -> Result<(), Self::Error>;
    async fn write_all(&mut self, buf: &[u8]) -> Result<(), Self::Error>;
    async fn read_some(&mut self, buf: &mut [u8], timeout: Option<core::time::Duration>) -> Result<usize, Self::Error>;
}

pub struct MicroClient<'a, P> {
    subscriptions: Subscribe<'a>,
    platform: &'a mut P,
    transport_client: TransportClient,
    pending_pubacks: [Option<u16>; 3],
    tx_buffer: &'a mut [u8],
}

impl<'a, P: Platform> MicroClient<'a, P> {
    pub fn new(platform: &'a mut P, subscriptions: &'a[SubscribeFilter<'a>], max_packet_size: usize, tx_buffer: &'a mut [u8]) -> Self {
        MicroClient {
            platform,
            subscriptions: Subscribe::new(1, subscriptions),
            transport_client: TransportClient::new(max_packet_size),
            pending_pubacks: [None; 3],
            tx_buffer
        }
    }

    pub async fn publish_qos0(&mut self, topic: &str, payload: &[u8]) -> Result<(), Error<P>> {
        let mut cursor = WriteCursor::new(self.tx_buffer);
        Publish {
            pkid: 0,
            payload,
            qos: QoS::AtMostOnce,
            dup: false,
            retain: false,
            topic
        }.write(&mut cursor)?;
        self.platform.write_all(cursor.finish().0)?;
        Ok(())
    }

    pub async fn publish_qos1(&mut self, topic: &str, payload: &[u8]) -> Result<Option<u16>, Error<P>> {
    }
}
