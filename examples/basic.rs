use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use umqtt::nano_client::{ClientNotification, NanoClient};
use umqtt::packetview::connect::ConnectOptions;
use umqtt::time::Instant;

struct Platform {
    stream: Option<TcpStream>,
    epoch: tokio::time::Instant,
}

impl umqtt::nano_client::Platform for Platform {
    type Error = std::io::Error;

    async fn connect_transport(&mut self) -> Result<(), Self::Error> {
        self.stream = Some(TcpStream::connect(("test.mosquitto.org", 1883)).await?);
        Ok(())
    }

    async fn write_all(&mut self, buf: &[u8]) -> Result<(), Self::Error> {
        self.stream.as_mut().unwrap().write_all(buf).await
    }

    async fn read_some(
        &mut self,
        buf: &mut [u8],
        timeout: Option<Duration>,
    ) -> Result<Option<usize>, Self::Error> {
        println!("Reading with timeout {:?}", timeout);
        if let Some(timeout) = timeout {
            match tokio::time::timeout(timeout, self.stream.as_mut().unwrap().read(buf)).await {
                Ok(x) => Ok(Some(x?)),
                Err(_) => Ok(None),
            }
        } else {
            self.stream.as_mut().unwrap().read(buf).await.map(Some)
        }
    }

    fn now(&mut self) -> Instant {
        Instant::from_duration_since_epoch(tokio::time::Instant::now() - self.epoch)
    }
}

async fn run_client<P: umqtt::nano_client::Platform>(
    client: &mut NanoClient<'_, P>,
) -> Result<(), umqtt::nano_client::Error<P>> {
    let start_time = tokio::time::Instant::now();
    loop {
        let tick_result = client.next_notification().await?;
        match &tick_result {
            ClientNotification::TransportNotification(notif) => {
                if let umqtt::transport_client::Notification::Publish(publish) = &notif.notification
                {
                    println!(
                        "Publish received on topic '{}': {:?}",
                        publish.topic, publish.payload
                    );
                }
            }
            ClientNotification::SendPing => {
                println!("Sending ping");
            }
        }
        println!("Uptime: {:?}", (tokio::time::Instant::now() - start_time));
        // This needs to be stored in a seperate variable, otherwise the borrow-checker will complain!
        let tick_result = tick_result.complete();
        client.complete_notification(tick_result).await?;
    }
}

#[tokio::main]
async fn main() {
    let options = ConnectOptions {
        client_id: "umqttbasicclient",
        keep_alive: 10,
        clean_session: true,
        ..Default::default()
    };

    let mut tx_buffer = Box::new([0u8; 1024]);
    let mut rx_buffer = Box::new([0u8; 1024]);
    let platform = Platform {
        stream: None,
        epoch: tokio::time::Instant::now(),
    };
    let mut client = NanoClient::new(platform, tx_buffer.as_mut_slice(), rx_buffer.as_mut_slice());
    loop {
        let connack = client.connect(&options).await;
        if let Ok(connack) = connack {
            if connack.code.is_success() {
                println!("Connected");
                println!("{:?}", run_client(&mut client).await);
            } else {
                println!("Failed to connect, server returned {:?}", connack);
            }
        } else {
            println!("Failed to connect...");
        }
        tokio::time::sleep(Duration::from_secs(20)).await;
    }
}
