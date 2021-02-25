use machine::*;
use miniconf::{
    embedded_nal::{IpAddr, Ipv4Addr},
    minimq::QoS,
    StringSet,
};
use serde::Deserialize;

#[macro_use]
extern crate log;

#[derive(Debug, Default, StringSet, Deserialize)]
struct AdditionalSettings {
    inner: u8,
}

#[derive(Debug, Default, StringSet, Deserialize)]
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
        CommitSetting,
        Complete,
    }
);

transitions!(TestState,
    [
      (Started, Advance) => SentSimpleSetting,
      (SentSimpleSetting, Advance) => SentInnerSetting,
      (SentInnerSetting, Advance) => CommitSetting,
      (CommitSetting, Advance) => Complete
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
    pub fn on_advance(self, _: Advance) -> CommitSetting {
        CommitSetting {}
    }
}

impl CommitSetting {
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

    let localhost = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

    // Construct a Minimq client to the broker for publishing requests.
    let mut client: minimq::MqttClient<minimq::consts::U256, _> = {
        let stack = std_embedded_nal::STACK.clone();
        minimq::MqttClient::new(localhost, "tester", stack).unwrap()
    };

    // Construct a settings configuration interface.
    let mut interface: miniconf::MqttInterface<Settings, _, minimq::consts::U256> = {
        let stack = std_embedded_nal::STACK.clone();
        let dut_client = minimq::MqttClient::new(localhost, "clientid", stack).unwrap();
        miniconf::MqttInterface::new(dut_client, "device", Settings::default()).unwrap()
    };

    // We will wait 100ms in between each state to allow the MQTT broker to catch up
    let mut state = TestState::started();
    let mut timer = Timer::new(std::time::Duration::from_millis(100));

    loop {
        // First, update our client's MQTT state.
        client
            .poll(|_, _, msg, _| {
                let msg = std::str::from_utf8(msg).unwrap();
                info!("Got: {:?}", msg);
            })
            .unwrap();

        // Next, service the settings interface and progress the test.
        let setting_update = interface.update().unwrap();
        match state {
            TestState::Started(_) => {
                // When first starting, let both clients connect and the timer elapse. Otherwise,
                // one may publish before both devices are fully connected.
                if timer.is_complete()
                    && client.is_connected().unwrap()
                    && interface.client(|iclient| iclient.is_connected().unwrap())
                {
                    // Subscribe to the default device log topic.
                    client.subscribe("device/log", &[]).unwrap();

                    // Send a request to set a property.
                    info!("Sending first settings value");
                    client
                        .publish(
                            "device/settings/data",
                            "500".as_bytes(),
                            QoS::AtMostOnce,
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
                    client
                        .publish(
                            "device/settings/more/inner",
                            "100".as_bytes(),
                            QoS::AtMostOnce,
                            &[],
                        )
                        .unwrap();
                    state = state.on_advance(Advance);
                    timer.restart();
                }
            }
            TestState::SentInnerSetting(_) => {
                // Finally, commit the settings so they become active.
                if timer.is_complete() || setting_update {
                    assert!(setting_update);
                    info!("Committing settings");
                    client
                        .publish("device/commit", "".as_bytes(), QoS::AtMostOnce, &[])
                        .unwrap();
                    state = state.on_advance(Advance);
                    timer.restart();
                }
            }
            TestState::CommitSetting(_) => {
                if setting_update {
                    info!("Settings commit detected");
                    state = state.on_advance(Advance);
                    timer.restart();
                }
                assert!(timer.is_complete() == false);
            }
            TestState::Complete(_) => {
                // Verify the settings all have the correct value.
                info!("Verifying settings: {:?}", interface.settings);
                assert!(interface.settings.data == 500);
                assert!(interface.settings.more.inner == 100);
                std::process::exit(0);
            }

            other => panic!("Undefined state: {:?}", other),
        }
    }
}
