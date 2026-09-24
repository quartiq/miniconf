//! A small settings command interface, implemented manually or with Tree.
#![no_std]

use core::hint::black_box;
use cortex_m_semihosting::{debug, hio};

#[path = "../../../examples/common.rs"]
mod common;
use common::Settings;

#[cfg_attr(feature = "tree", path = "tree.rs")]
#[cfg_attr(not(feature = "tree"), path = "manual.rs")]
mod engine;

#[derive(Debug, PartialEq, Eq)]
enum Error {
    Path,
    Unavailable,
    Access,
    Value,
}

fn command<'a>(
    settings: &mut Settings,
    command: &str,
    out: &'a mut [u8],
) -> Result<&'a [u8], Error> {
    let (path, value) = command
        .split_once('=')
        .map_or((command, None), |(path, value)| {
            (path.trim(), Some(value.trim()))
        });
    if !path.starts_with('/') {
        return Err(Error::Path);
    }
    let len = engine::exchange(settings, path, value, out)?;
    Ok(if value.is_some() { b"OK" } else { &out[..len] })
}

fn session(settings: &mut Settings, input: &str, mut reply: impl FnMut(&[u8])) {
    let mut out = [0; 32];
    for cmd in input.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        #[cfg(feature = "help")]
        let result = if cmd == "help" {
            reply(b"Read: /path; write: /path=JSON; help [/path]");
            engine::help("", &mut reply)
        } else if let Some(path) = cmd.strip_prefix("help ") {
            let path = path.trim();
            if path.starts_with('/') {
                engine::help(if path == "/" { "" } else { path }, &mut reply)
            } else {
                Err(Error::Path)
            }
        } else {
            command(settings, cmd, &mut out).map(&mut reply)
        };
        #[cfg(not(feature = "help"))]
        let result = command(settings, cmd, &mut out).map(&mut reply);
        if let Err(error) = result {
            reply(match error {
                Error::Path => b"ERR path",
                Error::Unavailable => b"ERR unavailable",
                Error::Access => b"ERR access",
                Error::Value => b"ERR value",
            });
        }
    }
}

fn workload(mut reply: impl FnMut(&[u8])) {
    let mut settings = black_box(Settings::new());
    session(
        &mut settings,
        black_box(include_str!("commands.txt")),
        &mut reply,
    );
    // A sensor sample arrives; calibration becomes unavailable.
    settings.temperature = black_box(Some(1.5));
    settings.calibration = None;
    session(
        &mut settings,
        black_box("/temp; /calibration/offset; /calibration/slope=0;"),
        &mut reply,
    );
    #[cfg(feature = "help")]
    session(
        &mut settings,
        black_box(include_str!("help-commands.txt")),
        &mut reply,
    );
}

fn stack_peak_bytes() -> usize {
    unsafe extern "C" {
        static __sheap: u32;
        static _stack_start: u32;
    }
    let mut cursor = (&raw const __sheap) as usize;
    let top = (&raw const _stack_start) as usize;
    while cursor < top
        && unsafe { core::ptr::read_volatile(cursor as *const u32) }
            == cortex_m_rt::STACK_PAINT_VALUE
    {
        cursor += core::mem::size_of::<u32>();
    }
    top - cursor
}

pub fn run() -> ! {
    let mut stdout = hio::hstdout().unwrap();
    stdout.write_all(b"BEGIN\n").unwrap();
    workload(|reply| {
        stdout.write_all(reply).unwrap();
        stdout.write_all(b"\n").unwrap();
    });
    let stack = stack_peak_bytes();
    stdout.write_all(b"END\n").unwrap();
    let mut report = *b"RESULT stack_peak=0x00000000\n";
    for (i, digit) in report[20..28].iter_mut().enumerate() {
        *digit = b"0123456789abcdef"[(stack >> (4 * (7 - i))) & 15];
    }
    stdout.write_all(&report).unwrap();
    debug::exit(debug::EXIT_SUCCESS);
    loop {
        core::hint::spin_loop();
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;

    #[test]
    fn transcript() {
        let mut replies = std::vec::Vec::new();
        workload(|reply| {
            replies.extend_from_slice(reply);
            replies.push(b'\n');
        });
        let mut expected = std::string::String::from(include_str!("replies.txt"));
        if cfg!(feature = "help") {
            expected.push_str(include_str!("help-replies.txt"));
        }
        assert_eq!(std::str::from_utf8(&replies).unwrap(), expected);
    }

    #[test]
    fn rejected_values_and_framing() {
        let mut settings = Settings::new();
        let mut out = [0; 32];
        for (cmd, expected) in [
            ("/output/dac/0=4095", Ok(b"OK".as_slice())),
            ("/output/dac/0=4096", Err(Error::Access)),
            ("/output/dac/0", Ok(b"4095")),
            ("/output/attenuation/0=32768", Err(Error::Value)),
            ("/output/attenuation/0", Ok(b"0")),
            ("/control/mode=\"unknown\"", Err(Error::Value)),
            ("/control/mode", Ok(b"\"Run\"")),
            ("/control/enabled/extra", Err(Error::Path)),
            ("/output/dac/2", Err(Error::Path)),
            ("control/enabled", Err(Error::Path)),
            ("/control/enabled=", Err(Error::Value)),
            ("/serial=7", Err(Error::Access)),
            ("/serial", Ok(b"4660")),
            // JSON finalization follows the write, as in the Tree JSON API.
            ("/control/enabled=false garbage", Err(Error::Value)),
            ("/control/enabled", Ok(b"false")),
        ] {
            assert_eq!(command(&mut settings, cmd, &mut out), expected, "{cmd}");
        }
        let mut replies = std::vec::Vec::new();
        session(
            &mut settings,
            " ; /control/enabled = true ; /control/enabled; ",
            |r| replies.push(r.to_vec()),
        );
        assert_eq!(replies, [b"OK".as_slice(), b"true"]);
    }

    #[cfg(feature = "help")]
    #[test]
    fn help_paths() {
        let mut settings = Settings::new();
        let mut replies = std::vec::Vec::new();
        session(
            &mut settings,
            "help /missing; help /output/dac/2; help /temp/extra; help relative; helper;",
            |r| replies.push(r.to_vec()),
        );
        assert_eq!(replies, [b"ERR path"; 5]);
        let mut root = std::vec::Vec::new();
        engine::help("", |r| root.push(r.to_vec())).unwrap();
        replies.clear();
        session(&mut settings, "help /", |r| replies.push(r.to_vec()));
        assert_eq!(replies, root);
    }
}
