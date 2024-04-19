use machine::*;
use miniconf::Tree;
use minimq::Publication;
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
    #[tree(depth = 1)]
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
    }
);

transitions!(TestState,
    [
      (Started, Advance) => SentSimpleSetting,
      (SentSimpleSetting, Advance) => SentInnerSetting
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
    let mut buffer = [0u8; 1024];
    let localhost: minimq::embedded_nal::IpAddr = "127.0.0.1".parse().unwrap();
    let mut mqtt: minimq::Minimq<'_, _, _, minimq::broker::IpBroker> = minimq::Minimq::new(
        Stack,
        StandardClock::default(),
        minimq::ConfigBuilder::new(localhost.into(), &mut buffer),
    );

    let mut buffer = [0u8; 1024];
    let localhost: minimq::embedded_nal::IpAddr = "127.0.0.1".parse().unwrap();

    // Construct a settings configuration interface.
    let mut interface: miniconf_mqtt::MqttClient<'_, _, _, _, minimq::broker::IpBroker, 2> =
        miniconf_mqtt::MqttClient::new(
            Stack,
            "device",
            StandardClock::default(),
            minimq::ConfigBuilder::new(localhost.into(), &mut buffer),
        )
        .unwrap();

    // We will wait 100ms in between each state to allow the MQTT broker to catch up
    let mut state = TestState::started();
    let mut timer = Timer::new(std::time::Duration::from_millis(100));

    let mut settings = Settings::default();

    loop {
        // First, update our client's MQTT state.
        mqtt.poll(|_, _, msg, _| {
            let msg = std::str::from_utf8(msg).unwrap();
            info!("Got: {:?}", msg);
        })
        .unwrap();

        // Next, service the settings interface and progress the test.
        let setting_update = interface.update(&mut settings).unwrap();
        match state {
            TestState::Started(_) => {
                if timer.is_complete() && mqtt.client().is_connected() {
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
                if timer.is_complete() || setting_update {
                    assert!(setting_update);
                    // Verify the settings all have the correct value.
                    info!("Verifying settings: {:?}", settings);
                    assert_eq!(settings.data, 500);
                    assert_eq!(settings.more.inner, 100);
                    std::process::exit(0);
                }
            }

            other => panic!("Undefined state: {:?}", other),
        }
    }
}
