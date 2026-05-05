use embedded_io_adapters::tokio_1::FromTokio;
use miniconf_mqtt::{Error, Event, Miniconf};
use minimq::{ConfigBuilder, Error as MqttError};

#[path = "../../miniconf/examples/common.rs"]
mod common;

const BROKER: &str = "127.0.0.1:1883";
const PREFIX: &str = "test/common";

#[tokio::main]
async fn main() -> Result<(), Box<dyn core::error::Error>> {
    env_logger::init();
    defmt2log::init_from_current_exe()?;

    let mut buffer = vec![0; 4096];
    let (mut mm2, mut session) =
        Miniconf::new(PREFIX, ConfigBuilder::from_buffer(&mut buffer, 1024)?)?;
    let mut settings = common::Settings::new();
    println!("Serving common fixture on {PREFIX}");

    loop {
        let io = tokio::net::TcpStream::connect(BROKER).await?;
        let event = session.connect(FromTokio::new(io)).await?;
        mm2.startup(&mut session, &settings, event).await?;
        println!("{:?}", event);

        loop {
            match mm2.serve(&mut session, &mut settings, |_| ()).await {
                Ok(Event::Unhandled(())) => {}
                Ok(Event::Changed(idx)) => {
                    println!("Settings updated: {idx:?}");
                }
                Err(Error::Mqtt(MqttError::Disconnected)) => break,
                Err(err) => panic!("{err}"),
            }
        }
    }
}
