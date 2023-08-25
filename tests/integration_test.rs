#![cfg(feature = "mqtt-client")]

use machine::*;
use miniconf::Tree;
use minimq::{types::TopicFilter, Publication};
use std_embedded_nal::Stack;
use std_embedded_time::StandardClock;

#[macro_use]
extern crate log;

#[derive(Clone, Debug, Default, Tree)]
struct AdditionalSettings {
    inner: u8,
}

#[derive(Clone, Debug, Default, Tree)]
struct Settings {
    data: u32,
    #[tree()]
    more: AdditionalSettings,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Advance;

machine!(
    #[derive(Debug)]
    enum TestState {
        Started,
        SentSimpleSetting,
        SentInnerSetting,
        Complete,
    }
);

transitions!(TestState,
    [
      (Started, Advance) => SentSimpleSetting,
      (SentSimpleSetting, Advance) => SentInnerSetting,
      (SentInnerSetting, Advance) => Complete
    ]
);

impl Started {
    pub fn on_advance(self, _: Advance) -> SentSimpleSetting {
        SentSimpleSetting {}
    }
}

impl SentSimpleSetting {
    pub fn on_advance(self, _: Advance) -> SentInnerSetting {
        SentInnerSetting {}
    }
}

impl SentInnerSetting {
    pub fn on_advance(self, _: Advance) -> Complete {
        Complete {}
    }
}

struct Timer {
    started: std::time::Instant,
    duration: std::time::Duration,
}

impl Timer {
    pub fn new(timeout: std::time::Duration) -> Self {
        Self {
            started: std::time::Instant::now(),
            duration: timeout,
        }
    }

    pub fn restart(&mut self) {
        self.started = std::time::Instant::now();
    }

    pub fn is_complete(&self) -> bool {
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.started);
        elapsed >= self.duration
    }
}

#[test]
fn main() -> std::io::Result<()> {
    env_logger::init();

    // Construct a Minimq client to the broker for publishing requests.
    let mut rx_buffer = [0u8; 512];
    let mut tx_buffer = [0u8; 512];
    let mut session = [0u8; 512];
    let mut will = [0u8; 64];
    let localhost: minimq::embedded_nal::IpAddr = "127.0.0.1".parse().unwrap();
    let mut mqtt: minimq::Minimq<'_, _, _, minimq::broker::IpBroker> = minimq::Minimq::new(
        Stack::default(),
        StandardClock::default(),
        minimq::Config::new(localhost.into(), &mut rx_buffer, &mut tx_buffer)
            .session_state(&mut session)
            .will_buffer(&mut will)
            .keepalive_interval(60),
    );

    // Construct a settings configuration interface.
    let mut rx_buffer = [0u8; 512];
    let mut tx_buffer = [0u8; 512];
    let mut session = [0u8; 512];
    let localhost: minimq::embedded_nal::IpAddr = "127.0.0.1".parse().unwrap();

    // Construct a settings configuration interface.
    let mut interface: miniconf::MqttClient<'_, _, _, _, minimq::broker::IpBroker, 2> =
        miniconf::MqttClient::new(
            Stack::default(),
            "device",
            StandardClock::default(),
            Settings::default(),
            minimq::Config::new(localhost.into(), &mut rx_buffer, &mut tx_buffer)
                .client_id("tester")
                .unwrap()
                .session_state(&mut session)
                .keepalive_interval(60),
        )
        .unwrap();

    // We will wait 100ms in between each state to allow the MQTT broker to catch up
    let mut state = TestState::started();
    let mut timer = Timer::new(std::time::Duration::from_millis(100));

    loop {
        // First, update our client's MQTT state.
        mqtt.poll(|_, _, msg, _| {
            let msg = std::str::from_utf8(msg).unwrap();
            info!("Got: {:?}", msg);
        })
        .unwrap();

        // Next, service the settings interface and progress the test.
        let setting_update = interface.update().unwrap();
        match state {
            TestState::Started(_) => {
                if timer.is_complete() && mqtt.client().is_connected() {
                    // Subscribe to the default device log topic.
                    mqtt.client()
                        .subscribe(&[TopicFilter::new("device/log")], &[])
                        .unwrap();

                    // Send a request to set a property.
                    info!("Sending first settings value");
                    mqtt.client()
                        .publish(
                            Publication::new(b"500")
                                .topic("device/settings/data")
                                .finish()
                                .unwrap(),
                        )
                        .unwrap();
                    state = state.on_advance(Advance);
                    timer.restart();
                }
            }
            TestState::SentSimpleSetting(_) => {
                // Next, set a nested property.
                if timer.is_complete() || setting_update {
                    assert!(setting_update);
                    info!("Sending inner settings value");
                    mqtt.client()
                        .publish(
                            Publication::new(b"100")
                                .topic("device/settings/more/inner")
                                .finish()
                                .unwrap(),
                        )
                        .unwrap();
                    state = state.on_advance(Advance);
                    timer.restart();
                }
            }
            TestState::SentInnerSetting(_) => {
                // Finally, commit the settings so they become active.
                if timer.is_complete() {
                    state = state.on_advance(Advance);
                    timer.restart();
                }
            }
            TestState::Complete(_) => {
                // Verify the settings all have the correct value.
                info!("Verifying settings: {:?}", interface.settings());
                assert!(interface.settings().data == 500);
                assert!(interface.settings().more.inner == 100);
                std::process::exit(0);
            }

            other => panic!("Undefined state: {:?}", other),
        }
    }
}
