use embedded_io_async::Write;
use miniconf::{Internal, Meta, Schema};

use crate::MAX_DEPTH;

/// Describe a subtree, showing homogeneous children once as `0..N` (exclusive end).
///
/// Indented labels are path segments; ranges describe schema, not literal paths.
/// Semantics, edge metadata and node metadata remain distinct. Enable Miniconf's
/// `sem`, `meta-node`, and `meta-edge` features to retain them.
/// Schema possibilities do not establish runtime presence or writability.
/// `depth` counts child levels below `root`; use one for contextual help.
/// Path and depth errors are printed; output failures are returned.
pub async fn write_schema<W: Write>(
    writer: &mut W,
    schema: &'static Schema,
    root: &str,
    depth: usize,
) -> Result<(), W::Error> {
    let node = match schema.get(root) {
        Ok(node) => node.schema,
        Err(error) => return super::write_error(writer, error).await,
    };
    let (edge, _) = schema.get_meta(root).unwrap();
    writer
        .write_all(if root.is_empty() {
            b"(root)"
        } else {
            root.as_bytes()
        })
        .await?;
    describe(writer, node, edge.unwrap_or(&Meta::EMPTY)).await?;
    let mut stack = heapless::Vec::<_, MAX_DEPTH>::new();
    if depth > 0
        && let Some(children) = node.internal()
    {
        stack.push((children, 0)).unwrap();
    }
    while let Some((children, index)) = stack.last_mut() {
        let homogeneous = matches!(children, Internal::Homogeneous(_));
        if *index == if homogeneous { 1 } else { children.len().get() } {
            stack.pop();
            continue;
        }
        let child = children.get_schema(*index);
        let edge = children.get_edge_meta(*index);
        let number;
        let name = if homogeneous {
            number = heapless::format!(24; "0..{}", children.len()).unwrap();
            &number
        } else if let Some(name) = children.get_name(*index) {
            if name.is_empty() { "\"\"" } else { name }
        } else {
            number = heapless::format!(24; "{index}").unwrap();
            &number
        };
        *index += 1;
        for _ in 0..stack.len() {
            writer.write_all(b"  ").await?;
        }
        writer.write_all(name.as_bytes()).await?;
        describe(writer, child, edge).await?;
        if stack.len() < depth
            && let Some(children) = child.internal()
            && stack.push((children, 0)).is_err()
        {
            return super::write_error(writer, "schema exceeds MAX_DEPTH").await;
        }
    }
    Ok(())
}

async fn describe<W: Write>(writer: &mut W, node: &Schema, edge: &Meta) -> Result<(), W::Error> {
    writer
        .write_all(match node.internal() {
            None => b" [leaf]",
            Some(Internal::Named(_)) => b" [named]",
            Some(Internal::Numbered(_)) => b" [numbered]",
            Some(Internal::Homogeneous(_)) => b" [homogeneous]",
        })
        .await?;
    if let Some(sem) = node.sem().filter(|sem| !sem.is_empty()) {
        writer.write_all(b" [sem").await?;
        if let Some(ty) = sem.ty() {
            let mut text = heapless::format!(32; " ty={ty:?}").unwrap();
            text.make_ascii_lowercase();
            writer.write_all(text.as_bytes()).await?;
        }
        if sem.maybe_absent() {
            writer.write_all(b" maybe_absent").await?;
        }
        if sem.oneof() {
            writer.write_all(b" oneof").await?;
        }
        writer.write_all(b"]").await?;
    }
    for (label, meta) in [
        (b" [edge".as_slice(), edge),
        (b" [node".as_slice(), node.node_meta()),
    ] {
        if meta.iter().next().is_none() {
            continue;
        }
        writer.write_all(label).await?;
        for (key, value) in meta.iter() {
            writer.write_all(b" ").await?;
            writer.write_all(key.as_bytes()).await?;
            writer.write_all(b"=\"").await?;
            for ch in value.chars() {
                let text = heapless::format!(12; "{}", ch.escape_debug()).unwrap();
                writer.write_all(text.as_bytes()).await?;
            }
            writer.write_all(b"\"").await?;
        }
        writer.write_all(b"]").await?;
    }
    writer.write_all(b"\r\n").await
}
