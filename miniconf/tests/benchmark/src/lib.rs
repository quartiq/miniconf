#![no_std]

use core::hint::black_box;

use cortex_m_semihosting::{debug, hprintln};

pub mod codec;
pub mod manual_engine;
pub mod miniconf_engine;
pub mod settings;

use codec::Response;
use settings::Settings;

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
    "/foo",
    "/foo=true",
    "/foo",
    "/option=11",
    "/option",
    "/option=null",
    "/option",
    "/option_tree=13",
    "/option_tree",
    "/option_tree=-1",
    "/option_tree",
    "/enum_tree/C/0/a=12",
    "/enum_tree/C/0/b=21",
    "/enum_tree/C/1/a=-7",
    "/enum_tree/C/1/b=9",
    "/enum_tree/C/0/a",
    "/enum_tree/C/0/b",
    "/enum_tree/C/1/a",
    "/enum_tree/C/1/b",
    "/struct_tree/a=9",
    "/struct_tree/a",
    "/struct_tree/b=7",
    "/struct_tree/b",
    "/array_tree/1=-2",
    "/array_tree/0=3",
    "/array_tree/0",
    "/array_tree/1",
    "/array_tree2/0/a=12",
    "/array_tree2/0/b=21",
    "/array_tree2/1/a=-7",
    "/array_tree2/1/b=9",
    "/array_tree2/0/a",
    "/array_tree2/0/b",
    "/array_tree2/1/a",
    "/array_tree2/1/b",
    "/tuple_tree/0=22",
    "/tuple_tree/1/a=5",
    "/tuple_tree/1/b=14",
    "/tuple_tree/0",
    "/tuple_tree/1/a",
    "/tuple_tree/1/b",
    "/option_tree2/a=6",
    "/option_tree2/b=8",
    "/option_tree2/a",
    "/option_tree2/b",
    "/array_option_tree/0/a",
    "/array_option_tree/0/b",
    "/array_option_tree/0/a=1",
    "/array_option_tree/0/b=1",
    "/array_option_tree/1/a=4",
    "/array_option_tree/1/b=10",
    "/array_option_tree/1/a",
    "/array_option_tree/1/b",
    "/foo=false",
    "/foo",
];

pub trait Engine {
    const NAME: &'static str;
    type Error;
    fn new() -> Self;
    fn exec(&mut self, cmd: Command<'_>, out: &mut Response) -> Result<(), Self::Error>;
    fn settings(&self) -> &Settings;
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
    let mut out = Response::new();
    for _ in 0..outer_iters {
        for (i, line) in lines.iter().enumerate() {
            let index = i as u32;
            let cmd = parse(black_box(line)).map_err(|_| ValidationError::Parse(index))?;
            black_box(engine.exec(black_box(cmd), black_box(&mut out)))
                .map_err(|_| ValidationError::Set(index))?;
            black_box(out.as_bytes());
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
        let path = match cmd {
            Command::Set(path, _) => path,
            Command::Get(_) => continue,
        };
        engine
            .exec(cmd, &mut set_out)
            .map_err(|_| ValidationError::Set(index))?;
        engine
            .exec(Command::Get(path), &mut get_out)
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
        hprintln!("RESULT variant={}", E::NAME);
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

    run_parse_only(MIXED, 2_000);
    if let Err(err) = run_workload(&mut engine, MIXED, 2_000) {
        let (kind, index) = validation_code(err);
        hprintln!("RESULT variant={}", E::NAME);
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
    if let Err(err) = run_workload(&mut replay, MIXED, 2_000) {
        let (kind, index) = validation_code(err);
        hprintln!("RESULT variant={}", E::NAME);
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

    hprintln!("RESULT variant={}", E::NAME);
    hprintln!("RESULT validation=ok");
    hprintln!("RESULT final_state_eq={}", state_eq as u8);

    debug::exit(debug::EXIT_SUCCESS);
    loop {
        core::hint::spin_loop();
    }
}

pub fn run_baseline() -> ! {
    run_parse_only(MIXED, 2_000);

    hprintln!("RESULT variant=baseline_harness");
    hprintln!("RESULT validation=ok");
    hprintln!("RESULT final_state_eq=1");

    debug::exit(debug::EXIT_SUCCESS);
    loop {
        core::hint::spin_loop();
    }
}
