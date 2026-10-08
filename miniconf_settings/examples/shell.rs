use std::{
    future::ready,
    io::{self, BufRead, IsTerminal, Read, Write},
};

use futures::executor::block_on;
use miniconf::{TreeSchema, json_core};
use miniconf_settings::shell::{Command, ParseError, Terminal, write_schema, write_values};
use noline::{builder::EditorBuilder, error::NolineError};
use termion::raw::IntoRawMode;

mod common;

const COMMANDS: &[&str] = &["get", "set", "schema", "help"];
const HELP: &str = "get [path], set <path> <json>, schema [path], help\r\n\
    Settings are kept in memory; persistence is not configured.\r\n\
    Tab completes; repeat to list matches.\r\n";

struct Console<R, W>(R, W);

impl<R, W> embedded_io_async::ErrorType for Console<R, W> {
    type Error = io::Error;
}

impl<R: Read, W> embedded_io_async::Read for Console<R, W> {
    fn read(&mut self, bytes: &mut [u8]) -> impl Future<Output = io::Result<usize>> {
        ready(self.0.read(bytes))
    }
}

impl<R, W: Write> embedded_io_async::Write for Console<R, W> {
    fn write(&mut self, bytes: &[u8]) -> impl Future<Output = io::Result<usize>> {
        ready(self.1.write(bytes))
    }

    fn flush(&mut self) -> impl Future<Output = io::Result<()>> {
        ready(self.1.flush())
    }
}

fn dispatch(
    command: Result<Option<Command<'_>>, ParseError>,
    settings: &mut common::Settings,
    output: &mut impl Write,
) -> io::Result<()> {
    match command {
        Ok(Some(Command::Get(path))) => {
            block_on(write_values(
                &mut Console(io::empty(), &mut *output),
                settings,
                path,
                &mut [0; 512],
            ))?;
        }
        Ok(Some(Command::Schema(path))) => {
            block_on(write_schema(
                &mut Console(io::empty(), &mut *output),
                common::Settings::SCHEMA,
                path,
                usize::MAX,
            ))?;
        }
        Ok(Some(Command::Set { path, value })) => {
            match json_core::set(settings, path, value.as_bytes()) {
                Ok(_) => write!(output, "Set.\r\n")?,
                Err(error) => write!(output, "error: {error}\r\n")?,
            }
        }
        Ok(Some(Command::Help)) => output.write_all(HELP.as_bytes())?,
        Ok(Some(Command::Store | Command::ResetStored)) => {
            output.write_all(b"Persistence is not configured.\r\n")?;
        }
        Ok(None) => {}
        Err(error) => write!(output, "error: {error:?}\r\n")?,
    }
    output.flush()
}

fn main() -> io::Result<()> {
    env_logger::init();
    defmt2log::init_from_current_exe();
    let mut input = io::stdin().lock();
    let output = io::stdout();
    let mut settings = common::Settings::new();
    if !input.is_terminal() || !output.is_terminal() {
        for line in input.lines() {
            dispatch(Command::parse(&line?), &mut settings, &mut output.lock())?;
        }
        return Ok(());
    }
    let mut output = output.into_raw_mode()?;
    let mut buffer = [0; 1024];
    let mut history = [0; 4096];
    let mut editor = block_on(
        EditorBuilder::from_slice(&mut buffer)
            .with_slice_history(&mut history)
            .build_async(&mut Console(&mut input, &mut output)),
    )
    .map_err(|error| io::Error::other(format!("{error:?}")))?;
    let mut terminal = Terminal::default();
    loop {
        match block_on(terminal.readline(
            editor.line("> "),
            &mut Console(&mut input, &mut output),
            common::Settings::SCHEMA,
            COMMANDS,
        )) {
            Ok(Some(line)) => dispatch(Command::parse(line), &mut settings, &mut output)?,
            Err(NolineError::Aborted) => {}
            Ok(None) => return Ok(()),
            Err(error) => return Err(io::Error::other(format!("{error:?}"))),
        }
    }
}
