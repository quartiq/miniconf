use heapless::String;
use serde::Serialize;

pub(crate) fn json_text<const N: usize, T: Serialize>(value: &T) -> Result<String<N>, ()> {
    let mut buf = [0u8; N];
    let mut ser = serde_json_core::ser::Serializer::new(&mut buf);
    value.serialize(&mut ser).map_err(|_| ())?;
    let len = ser.end();
    let text = core::str::from_utf8(&buf[..len]).map_err(|_| ())?;
    let mut out = String::new();
    out.push_str(text).map_err(|_| ())?;
    Ok(out)
}
