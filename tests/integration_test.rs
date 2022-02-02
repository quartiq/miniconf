use machine::*;
use miniconf::{
    minimq::{QoS, Retain},
    Miniconf,
};
use serde::Deserialize;
use std_embedded_nal::Stack;
use std_embedded_time::StandardClock;

#[macro_use]
extern crate log;

#[derive(Clone, Debug, Default, Miniconf, Deserialize)]
struct AdditionalSettings {
    inner: u8,
}

#[derive(Clone, Debug, Default, Miniconf, Deserialize)]
struct Settings {
    data: u32,
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

    let localhost = "127.0.0.1".parse().unwrap();

    // Construct a Minimq client to the broker for publishing requests.
    let mut mqtt: minimq::Minimq<_, _, 256, 1> = miniconf::minimq::Minimq::new(
        localhost,
        "tester",
        Stack::default(),
        StandardClock::default(),
    )
    .unwrap();

    // Construct a settings configuration interface.
    let mut interface: miniconf::MqttClient<Settings, _, _, 256> = miniconf::MqttClient::new(
        Stack::default(),
        "",
        "device",
        localhost,
        StandardClock::default(),
        Settings::default(),
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
                if timer.is_complete() && mqtt.client.is_connected() {
                    // Subscribe to the default device log topic.
                    mqtt.client.subscribe("device/log", &[]).unwrap();

                    // Send a request to set a property.
                    info!("Sending first settings value");
                    mqtt.client
                        .publish(
                            "device/settings/data",
                            "500".as_bytes(),
                            QoS::AtMostOnce,
                            Retain::NotRetained,
                            &[],
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
                    mqtt.client
                        .publish(
                            "device/settings/more/inner",
                            "100".as_bytes(),
                            QoS::AtMostOnce,
                            Retain::NotRetained,
                            &[],
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
