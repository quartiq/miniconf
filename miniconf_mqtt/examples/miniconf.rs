use embedded_io_adapters::tokio_1::FromTokio;
use miniconf_mqtt::{Error, Event, Miniconf};
use minimq::{ConfigBuilder, Error as MqttError};

#[path = "../../miniconf/examples/common.rs"]
mod common;

const BROKER: &str = "127.0.0.1:1883";
const PREFIX: &str = "test/common";

#[tokio::main]
async fn main() -> Result<(), Box<dyn core::error::Error>> {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init();
    defmt2log::init_from_current_exe();

    let broker = std::env::args().nth(1).unwrap_or_else(|| BROKER.into());
    let mut buffer = vec![0; 4096];
    let (mut miniconf, mut session) =
        Miniconf::new(PREFIX, ConfigBuilder::from_buffer(&mut buffer, 1024)?)?;
    let mut settings = common::Settings::new();
    defmt::info!("serving common fixture prefix={=str}", PREFIX);
    defmt::info!(
        "try: miniconf --broker {=str} {=str} /control/enabled",
        broker.as_str(),
        PREFIX
    );
    defmt::info!(
        "try: miniconf --broker {=str} {=str} /control/enabled=false",
        broker.as_str(),
        PREFIX
    );

    loop {
        defmt::info!("connecting to mqtt://{=str}", broker.as_str());
        let io = tokio::net::TcpStream::connect(&broker).await?;
        let mut connection = session.connect(FromTokio::new(io)).await?;
        miniconf.startup(&mut connection, &settings).await?;
        defmt::info!(
            "mqtt session ready event={}",
            defmt::Debug2Format(&connection.connect_event())
        );

        loop {
            match miniconf.serve(&mut connection, &mut settings, |_| ()).await {
                Ok(Event::Unhandled(())) => {}
                Ok(Event::Changed(idx)) => {
                    defmt::info!("settings updated key={}", idx);
                }
                Err(Error::Mqtt(MqttError::Disconnected)) => {
                    defmt::warn!("mqtt disconnected; reconnecting");
                    break;
                }
                Err(err) => panic!("{err}"),
            }
        }
    }
}
