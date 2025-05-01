use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use umqtt::packetview::connect::ConnectOptions;
use umqtt::packetview::puback::PubAck;
use umqtt::packetview::QoS;
use umqtt::packetview::subscribe::{Subscribe, SubscribeFilter};
use umqtt::transport_client::{Notification, TransportClient};

pub struct RxBuffer<const N: usize> {
    buffer: [u8; N],
    pos: usize,
}

impl<const N: usize> RxBuffer<N> {
    pub fn new() -> Self {
        Self {
            buffer: [0u8; N],
            pos: 0,
        }
    }

    pub fn written_slice(&self) -> &[u8] {
        &self.buffer[..self.pos]
    }

    pub fn writable_slice(&mut self) -> &mut [u8] {
        &mut self.buffer[self.pos..]
    }

    pub fn advance(&mut self, amount: usize) {
        self.pos += amount;
    }

    pub fn remove(&mut self, amount: usize) {
        if amount >= self.pos {
            self.pos = 0;
            return;
        }
        self.buffer.copy_within(amount..self.pos, 0);
        self.pos -= amount;
    }
}

pub fn now(start_time: tokio::time::Instant) -> umqtt::time::Instant {
    umqtt::time::Instant::from_seconds_since_epoch((tokio::time::Instant::now() - start_time).as_secs())
}

#[tokio::main]
async fn main() {
    let options = ConnectOptions {
        client_id: "umqtt-basic-client",
        keep_alive: 10,
        clean_session: true,
        ..Default::default()
    };

    let mut transport_client = TransportClient::new(256*1024);
    let mut tx_buffer = Box::new([0u8; 1024]);

    let start_time = tokio::time::Instant::now();
    let mut connection = TcpStream::connect(("test.mosquitto.org", 1883)).await.unwrap();
    transport_client.on_transport_opened(&options);
    let written = options.as_connect().write(tx_buffer.as_mut_slice()).unwrap();
    connection.write_all(&tx_buffer.as_slice()[..written]).await.unwrap();
    transport_client.on_packet_sent(now(start_time));

    let mut rx_buffer = RxBuffer::<1024>::new();
    loop {
        if let Ok(Some(n)) = transport_client.on_bytes_received(rx_buffer.written_slice()) {
            println!("{:?}", n.0);
            match n.0 {
                Notification::ConnAck(_) => {
                    let filters = [SubscribeFilter::new("/andwass/#", QoS::AtLeastOnce)];
                    let written = Subscribe::new(1, &filters).write(&mut tx_buffer.as_mut_slice()).unwrap();
                    connection.write_all(&tx_buffer.as_slice()[..written]).await.unwrap();
                    transport_client.on_packet_sent(now(start_time));
                    println!("Connected!")
                },
                Notification::Publish(p) => {
                    if p.qos != QoS::AtMostOnce {
                        println!("Writing PUBACK with packet id {}", p.pkid);
                        let written = PubAck::new(p.pkid).write(&mut tx_buffer.as_mut_slice()).unwrap();
                        connection.write_all(&tx_buffer.as_slice()[..written]).await.unwrap();
                        transport_client.on_packet_sent(now(start_time));
                    }
                },
                Notification::Disconnected => return,
                _ => {}
            }
            rx_buffer.remove(n.1);
        }
        else {
            let timeout_in = transport_client.next_ping_in(now(start_time)).unwrap();
            let res = tokio::time::timeout(timeout_in, connection.read(rx_buffer.writable_slice())).await;
            match res {
                Err(_) => {
                    println!("Writing ping");
                    connection.write_all(&[0xC0, 0]).await.unwrap();
                    transport_client.on_packet_sent(now(start_time));
                },
                Ok(Err(_)) => {
                    println!("Disconnected")
                },
                Ok(Ok(n)) => rx_buffer.advance(n),
            }
        }
    }
}