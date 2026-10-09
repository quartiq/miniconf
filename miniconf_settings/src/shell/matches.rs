use embedded_io_async::Write;

use super::completion::Candidates;

const MAX_MATCHES: usize = 32;
const MAX_ROWS: usize = 4;

pub(super) async fn write_matches<W: Write>(
    writer: &mut W,
    candidates: Candidates<'_>,
    columns: usize,
) -> Result<(), W::Error> {
    if let Candidates::Children(_, "") = candidates
        && let Some(len) = candidates.numeric_len()
        && len > MAX_MATCHES
    {
        let hint = heapless::format!(64; "0..={}\r\n", len - 1).unwrap();
        return writer.write_all(hint.as_bytes()).await;
    }
    let names = candidates.tokens().filter_map(|(mut name, branch)| {
        if name.is_empty() {
            name.push_str("\"\"").ok()?;
        }
        if branch {
            name.push('/').ok()?;
        }
        Some(name)
    });
    let (count, longest) = names
        .clone()
        .take(MAX_MATCHES + 1)
        .fold((0, 0), |(count, longest), name| {
            (count + 1, longest.max(name.chars().count()))
        });
    if count == 0 {
        return writer.write_all(b"no matches\r\n").await;
    }
    let width = columns.clamp(1, 80).saturating_sub(1).max(1);
    let cell = (longest + 2).min(width);
    let per_row = (width / cell).max(1);
    let shown = count.min(MAX_MATCHES.min(per_row * MAX_ROWS));
    for (index, name) in names.take(shown).enumerate() {
        let length = name.chars().count();
        let limit = cell.saturating_sub(2).max(1);
        let printed = if length > limit {
            let end = name.char_indices().nth(limit - 1).unwrap().0;
            writer.write_all(name[..end].as_bytes()).await?;
            writer.write_all("…".as_bytes()).await?;
            limit
        } else {
            writer.write_all(name.as_bytes()).await?;
            length
        };
        if (index + 1) % per_row == 0 || index + 1 == shown {
            writer.write_all(b"\r\n").await?;
        } else {
            writer.write_all(&[b' '; 80][..cell - printed]).await?;
        }
    }
    if count > shown {
        writer.write_all(b"... narrow the prefix\r\n").await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    extern crate std;

    use futures::executor::block_on;
    use miniconf::TreeSchema;
    use std::vec::Vec;

    use super::*;

    #[test]
    fn filtered_columns_and_row_limit() {
        let commands = &["get", "set", "schema", "store", "reset", "help"];
        let mut output = Vec::new();
        block_on(write_matches(
            &mut output,
            Candidates::Commands(commands, "s"),
            80,
        ))
        .unwrap();
        assert_eq!(output, b"set     schema  store\r\n");
        output.clear();
        block_on(write_matches(
            &mut output,
            Candidates::Commands(commands, ""),
            10,
        ))
        .unwrap();
        assert_eq!(
            output,
            b"get\r\nset\r\nschema\r\nstore\r\n... narrow the prefix\r\n"
        );
        output.clear();
        block_on(write_matches(
            &mut output,
            Candidates::Commands(commands, "schema"),
            5,
        ))
        .unwrap();
        assert_eq!(output, "s…\r\n".as_bytes());
    }

    #[test]
    fn indexed_matches_show_branches_and_large_ranges() {
        for (schema, expected) in [
            (<[u16; 2]>::SCHEMA, "0  1\r\n"),
            (<(u16, [u16; 2])>::SCHEMA, "0   1/\r\n"),
            (<[u16; 1000]>::SCHEMA, "0..=999\r\n"),
        ] {
            let mut output = Vec::new();
            let (_, candidates) = Candidates::path(schema, "/", 1).unwrap();
            block_on(write_matches(&mut output, candidates, 80)).unwrap();
            assert_eq!(output, expected.as_bytes());
        }
    }
}
