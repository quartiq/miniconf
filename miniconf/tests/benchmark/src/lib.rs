#![no_std]

use core::hint::black_box;
use core::ptr::read_volatile;

use cortex_m_rt::STACK_PAINT_VALUE;
use cortex_m_semihosting::{debug, hprintln};
pub mod codec;
pub mod manual_engine;
pub mod miniconf_engine;
pub mod settings;

use codec::Response;

#[derive(Copy, Clone)]
pub enum Command<'a> {
    Get(&'a str),
    Set(&'a str, &'a str),
}

#[derive(Copy, Clone, Debug)]
pub enum ParseError {
    Empty,
    MissingPath,
    MissingValue,
}

pub fn parse(line: &str) -> Result<Command<'_>, ParseError> {
    if line.is_empty() {
        return Err(ParseError::Empty);
    }
    let bytes = line.as_bytes();
    if let Some(eq) = bytes.iter().position(|b| *b == b'=') {
        let path = line.get(..eq).ok_or(ParseError::MissingPath)?;
        let value = line.get(eq + 1..).ok_or(ParseError::MissingValue)?;
        if path.is_empty() || !path.as_bytes().starts_with(b"/") {
            return Err(ParseError::MissingPath);
        }
        if value.is_empty() {
            return Err(ParseError::MissingValue);
        }
        Ok(Command::Set(path, value))
    } else {
        if !bytes.starts_with(b"/") {
            return Err(ParseError::MissingPath);
        }
        Ok(Command::Get(line))
    }
}

pub const MIXED: &[&str] = &[
    "/serial",
    "/control/enabled",
    "/control/enabled=false",
    "/control/enabled",
    "/output/dac/0",
    "/output/dac/0=2048",
    "/output/dac/1=4095",
    "/output/dac/0",
    "/output/dac/1",
    "/output/attenuation/0=-3",
    "/output/attenuation/1=6",
    "/output/attenuation/0",
    "/output/attenuation/1",
    "/calibration/offset=-9",
    "/calibration/slope=24",
    "/calibration/offset",
    "/calibration/slope",
    "/control/enabled=true",
    "/control/enabled",
];

// This harness is primarily for code size and representative path coverage.
// Stack depth is iteration-invariant here, so keep the QEMU workload short.
const OUTER_ITERS: u32 = 32;

pub trait Engine {
    type Error;
    fn new() -> Self;
    fn set(&mut self, path: &str, value: &str) -> Result<(), Self::Error>;
    fn get(&self, path: &str, out: &mut Response) -> Result<(), Self::Error>;
    fn settings(&self) -> &settings::Settings;
}

fn stack_peak_bytes() -> usize {
    unsafe extern "C" {
        static __sheap: u32;
        static _stack_start: u32;
    }

    let mut cursor = (&raw const __sheap) as usize;
    let top = (&raw const _stack_start) as usize;
    while cursor < top && unsafe { read_volatile(cursor as *const u32) } == STACK_PAINT_VALUE {
        cursor += core::mem::size_of::<u32>();
    }
    top - cursor
}

fn run_parse_only(lines: &[&str], outer_iters: u32) {
    for _ in 0..outer_iters {
        for line in lines {
            let _ = black_box(parse(black_box(line)));
        }
    }
}

fn run_workload<E: Engine>(
    engine: &mut E,
    lines: &[&str],
    outer_iters: u32,
) -> Result<(), ValidationError> {
    for _ in 0..outer_iters {
        for (i, line) in lines.iter().enumerate() {
            let index = i as u32;
            let cmd = parse(black_box(line)).map_err(|_| ValidationError::Parse(index))?;
            match black_box(cmd) {
                Command::Get(path) => {
                    let mut out = Response::new();
                    black_box(engine.get(black_box(path), black_box(&mut out)))
                        .map_err(|_| ValidationError::Get(index))?;
                    black_box(out.as_bytes());
                }
                Command::Set(path, value) => {
                    black_box(engine.set(black_box(path), black_box(value)))
                        .map_err(|_| ValidationError::Set(index))?;
                }
            }
        }
    }
    Ok(())
}

#[allow(dead_code)]
enum ValidationError {
    Parse(u32),
    Set(u32),
    Get(u32),
    Mismatch(u32),
}

fn validate_set_roundtrip<E: Engine>(engine: &mut E) -> Result<(), ValidationError> {
    let mut set_out = Response::new();
    let mut get_out = Response::new();
    for (i, line) in MIXED.iter().enumerate() {
        let index = i as u32;
        let cmd = parse(line).map_err(|_| ValidationError::Parse(index))?;
        let (path, value) = match cmd {
            Command::Set(path, value) => (path, value),
            Command::Get(_) => continue,
        };
        engine
            .set(path, value)
            .map_err(|_| ValidationError::Set(index))?;
        engine
            .get(path, &mut set_out)
            .map_err(|_| ValidationError::Get(index))?;
        engine
            .get(path, &mut get_out)
            .map_err(|_| ValidationError::Get(index))?;
        if set_out.as_bytes() != get_out.as_bytes() {
            return Err(ValidationError::Mismatch(index));
        }
    }
    Ok(())
}

fn validation_code(err: ValidationError) -> (&'static str, u32) {
    match err {
        ValidationError::Parse(i) => ("parse", i),
        ValidationError::Set(i) => ("set", i),
        ValidationError::Get(i) => ("get", i),
        ValidationError::Mismatch(i) => ("mismatch", i),
    }
}

pub fn run_engine<E: Engine>() -> ! {
    let mut engine = E::new();
    if let Err(err) = validate_set_roundtrip(&mut engine) {
        let (kind, index) = validation_code(err);
        hprintln!(
            "RESULT validation=set_get_roundtrip_failed kind={} index={}",
            kind,
            index
        );
        debug::exit(debug::EXIT_FAILURE);
        loop {
            core::hint::spin_loop();
        }
    }

    run_parse_only(MIXED, OUTER_ITERS);
    if let Err(err) = run_workload(&mut engine, MIXED, OUTER_ITERS) {
        let (kind, index) = validation_code(err);
        hprintln!(
            "RESULT validation=workload_failed kind={} index={}",
            kind,
            index
        );
        debug::exit(debug::EXIT_FAILURE);
        loop {
            core::hint::spin_loop();
        }
    }
    let mut replay = E::new();
    if let Err(err) = run_workload(&mut replay, MIXED, OUTER_ITERS) {
        let (kind, index) = validation_code(err);
        hprintln!(
            "RESULT validation=replay_failed kind={} index={}",
            kind,
            index
        );
        debug::exit(debug::EXIT_FAILURE);
        loop {
            core::hint::spin_loop();
        }
    }
    let state_eq = engine.settings() == replay.settings();

    hprintln!("RESULT validation=ok");
    hprintln!("RESULT final_state_eq={}", state_eq as u8);
    hprintln!("RESULT stack_peak={}", stack_peak_bytes());

    debug::exit(debug::EXIT_SUCCESS);
    loop {
        core::hint::spin_loop();
    }
}

pub fn run_baseline() -> ! {
    run_parse_only(MIXED, OUTER_ITERS);
    hprintln!("RESULT validation=ok");
    hprintln!("RESULT stack_peak={}", stack_peak_bytes());

    debug::exit(debug::EXIT_SUCCESS);
    loop {
        core::hint::spin_loop();
    }
}
