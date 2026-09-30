use embedded_io_async::{ErrorKind, Read, Write};
use heapless::{Deque, Vec};
use miniconf::Schema;
use noline::{
    editor::{Event, Line, OutputItem},
    error::NolineError,
    history::History,
    line_buffer::Buffer,
};

use super::{Completion, complete, write_schema};

/// Settings terminal interaction. Retains input queued during cursor queries.
/// Create a fresh terminal after a disconnect.
#[derive(Default)]
pub struct Terminal {
    pending: Deque<u8, 256>,
}

impl Terminal {
    /// Edit a command with completion, history, and repeated-Tab contextual help.
    ///
    /// The caller owns the editor buffers and dispatches the returned line. Help
    /// shows one schema level and restores the draft and cursor. The terminal must
    /// answer ANSI cursor queries; up to 256 input bytes can wait for an answer.
    /// Ctrl-C returns `Aborted`; Ctrl-D on an empty line or input EOF returns
    /// `None`. After an I/O error, reconnect before starting again.
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
                    let start = bytes.windows(2).rposition(|w| w == b"\x1b[");
                    if let Some(start) = start {
                        let fields = bytes[start + 2..bytes.len() - 1].split(|&b| b == b';');
                        if fields.clone().count() == 2
                            && fields
                                .clone()
                                .all(|s| !s.is_empty() && s.iter().all(u8::is_ascii_digit))
                        {
                            for &byte in &bytes[..start] {
                                self.pending
                                    .push_back(byte)
                                    .map_err(|_| ErrorKind::OutOfMemory)?;
                            }
                            for &byte in &bytes[start..] {
                                output(io, line.advance(byte)?).await?;
                            }
                            break;
                        }
                    }
                    for &byte in &bytes {
                        self.pending
                            .push_back(byte)
                            .map_err(|_| ErrorKind::OutOfMemory)?;
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
            tab = matches!(completion, Some(Completion::Help(_)));
            if let Some(Completion::Help(root)) = completion.as_ref()
                && previous_tab
            {
                output(io, line.suspend()?).await?;
                if let Some(root) = root {
                    write_schema(io, schema, &line.as_str()[root.clone()], 1).await?;
                } else {
                    for command in commands {
                        io.write_all(command.as_bytes()).await?;
                        io.write_all(b" ").await?;
                    }
                    io.write_all(b"\r\n").await?;
                }
                output(io, line.resume()?).await?;
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
    let mut result = Ok(false);
    for item in edits {
        if let Some(bytes) = item.get_bytes() {
            io.write_all(bytes).await?;
        }
        match item.event() {
            Some(Event::Submitted) => result = Ok(true),
            Some(Event::Aborted) => result = Err(NolineError::Aborted),
            _ => {}
        }
    }
    io.flush().await?;
    result
}
