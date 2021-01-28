use miniconf::{StringSet, embedded_nal::{IpAddr, Ipv4Addr}};
use machine::*;
use serde::Deserialize;

#[derive(Default, StringSet, Deserialize)]
struct AdditionalSettings {
    inner: u8,
}

#[derive(Default, StringSet, Deserialize)]
struct Settings {
    data: u32,
    more: AdditionalSettings,
}

machine! {
    enum TestState {
        Started,
        SentSimpleSetting,
        SentInnerSetting,
        CommitSetting,
        Complete
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Advance;

transitions! {TestState,
    [
      (Started, Advance) => SentSimpleSetting,
      (SentSimpleSetting, Advance) => SentInnerSetting,
      (SentInnerSetting, Advance) => CommitSetting,
      (CommitSetting, Advance) => Complete
    ]
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
        now.duration_since(self.started) >= self.duration
    }
}

#[test]
fn main() -> std::io::Result<()> {
    env_logger::init();

    // Construct a Minimq client to the broker for publishing requests.
    let mut client = {
        let stack = std_embedded_nal::STACK.clone();
        let localhost = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        minimq::MqttClient::new(localhost, "client", stack).unwrap()
    };

    let mut interface: miniconf::MqttInterface<Settings, _> = {
        let stack = std_embedded_nal::STACK.clone();
        miniconf::MqttInterface::new(stack, "test-device", Settings::default()).unwrap()
    };

    let mut state = TestState::started();
    let mut timer = Timer::new(std::time::Duration::from_millis(100));

    loop {
        let action = interface.update().unwrap();
        match state {
            TestState::Started(_) => {
                if client.is_connected().unwrap() && interface.client(|iclient| iclient.is_connected().unwrap()) {
                    // Send a request to set a property.
                    client.publish("test-device/settings/data", "500".as_bytes()).unwrap();
                    state = state.on_advance(Advance);
                    timer.restart();
                }
            },
            TestState::SentSimpleSetting(_) => {
                if timer.is_complete() {
                    client.publish("test-device/settings/more/inner", "100".as_bytes()).unwrap();
                    state = state.on_advance(Advance);
                    timer.restart();
                }
            },
            TestState::SentInnerSetting(_) => {
                if timer.is_complete() {
                    client.publish("test-device/commit", "".as_bytes()).unwrap();
                    state = state.on_advance(Advance);
                    timer.restart();
                }
            },
            TestState::CommitSetting(_) => {
                if action == miniconf::Action::CommitSettings {
                    state = state.on_advance(Advance);
                    timer.restart();
                }
                assert!(timer.is_complete() == false);
            },
            TestState::Complete(_) => {
                assert!(interface.settings.data == 500);
                assert!(interface.settings.more.inner == 100);
                std::process::exit(0);
            }
        }
    }

}
