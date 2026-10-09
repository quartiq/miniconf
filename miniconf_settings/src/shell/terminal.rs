use embedded_io_async::{ErrorKind, Read, Write};
use heapless::{Deque, Vec};
use miniconf::Schema;
use noline::{
    editor::{Event, Line, OutputItem},
    error::NolineError,
    history::History,
    line_buffer::Buffer,
};

use super::{Completion, complete, completion::Context, matches::write_matches};

/// Settings terminal interaction. Retains input queued during cursor queries.
/// Create a fresh terminal after a disconnect.
#[derive(Default)]
pub struct Terminal {
    pending: Deque<u8, 256>,
}

impl Terminal {
    /// Edit a command with completion, history, and repeated-Tab match listing.
    ///
    /// The caller owns the editor buffers and dispatches the returned line. Match
    /// listings are bounded and restore the draft and cursor. The terminal must
    /// answer ANSI cursor queries; up to 256 input bytes can wait for an answer.
    /// Ctrl-C returns `Aborted`; Ctrl-D on an empty line or input EOF returns
    /// `None`. After an I/O error, reconnect before starting again.
    /// Cancellation can interrupt output and discard queued input. Restore the
    /// terminal and create a fresh `Terminal` before starting another line.
    /// Application command names complete alongside settings commands; only
    /// `get`, `set`, and `schema` receive path completion.
    pub async fn readline<'a, 'prompt, B, H, I, IO>(
        &mut self,
        mut line: Line<'a, B, H, I>,
        io: &mut IO,
        schema: &'static Schema,
        commands: &[&str],
    ) -> Result<Option<&'a str>, NolineError>
    where
        B: Buffer,
        H: History,
        I: Iterator<Item = &'prompt str> + Clone,
        IO: Read + Write,
    {
        output(io, line.start()).await?;
        let mut tab = false;
        let mut columns = None;
        loop {
            let Some(cursor) = line.cursor() else {
                // Cursor replies share the input stream with queued keystrokes.
                let mut bytes = Vec::<u8, 256>::new();
                loop {
                    let Some(byte) = read_byte(io).await? else {
                        return Ok(None);
                    };
                    bytes.push(byte).map_err(|_| ErrorKind::OutOfMemory)?;
                    if byte != b'R' {
                        continue;
                    }
                    let start = bytes
                        .windows(2)
                        .rposition(|w| w == b"\x1b[")
                        .filter(|&start| {
                            // Frame a CSI ending in R; Noline validates its parameters.
                            bytes[start + 2..bytes.len() - 1]
                                .iter()
                                .all(|byte| !(0x40..=0x7e).contains(byte))
                        });
                    for &byte in &bytes[..start.unwrap_or(bytes.len())] {
                        self.pending
                            .push_back(byte)
                            .map_err(|_| ErrorKind::OutOfMemory)?;
                    }
                    if let Some(start) = start {
                        if columns.is_none() {
                            columns = core::str::from_utf8(&bytes[start + 2..bytes.len() - 1])
                                .ok()
                                .and_then(|reply| reply.split_once(';')?.1.parse().ok());
                        }
                        for &byte in &bytes[start..] {
                            output(io, line.advance(byte)?).await?;
                        }
                        break;
                    }
                    bytes.clear();
                }
                continue;
            };
            let byte = match self.pending.pop_front() {
                Some(byte) => byte,
                None => match read_byte(io).await? {
                    Some(byte) => byte,
                    None => return Ok(None),
                },
            };
            if byte == 4 && line.as_str().is_empty() {
                return Ok(None);
            }
            let completion = if byte == b'\t' {
                complete(schema, line.as_str(), cursor, commands)
            } else {
                None
            };
            let previous_tab = tab;
            tab = matches!(completion, Some(Completion::Matches(_)));
            if tab && previous_tab {
                output(io, line.suspend()?).await?;
                let context = Context::new(schema, line.as_str(), cursor, commands).unwrap();
                write_matches(io, context.candidates, columns.unwrap_or(80)).await?;
                output(io, line.resume()?).await?;
                columns = None;
                continue;
            }
            let edits = match completion {
                Some(Completion::Replace(range, text)) => line.replace(range, &text),
                _ => line.advance(byte),
            }?;
            if output(io, edits).await? {
                break;
            }
        }
        Ok(Some(line.into_str()))
    }
}

async fn read_byte(io: &mut impl Read) -> Result<Option<u8>, NolineError> {
    let mut byte = [0];
    if io.read(&mut byte).await? == 0 {
        return Ok(None);
    }
    Ok(Some(byte[0]))
}

async fn output<'a>(
    io: &mut impl Write,
    edits: impl IntoIterator<Item = OutputItem<'a>>,
) -> Result<bool, NolineError> {
    let mut result = None;
    for item in edits {
        if let Some(bytes) = item.get_bytes() {
            io.write_all(bytes).await?;
        }
        result = Some(match item.event() {
            Some(Event::Submitted) => Ok(true),
            Some(Event::Aborted) => Err(NolineError::Aborted),
            _ => Ok(false),
        });
    }
    if result.is_some() {
        io.flush().await?;
    }
    result.unwrap_or(Ok(false))
}
