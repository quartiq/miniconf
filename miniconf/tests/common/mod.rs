#![allow(unused)]

use miniconf::{
    json, DescendError, IntoKeys, KeyError, Keys, Node, Packed, Path, Schema, Transcode,
    TreeDeserialize, TreeKey, TreeSerialize,
};

pub fn paths<const D: usize>(schema: &'static Schema) -> Vec<String> {
    assert!(schema
        .nodes::<_, D>()
        .exact_size()
        .collect::<Result<Vec<(Packed, _)>, _>>()
        .unwrap()
        .is_sorted());
    schema
        .nodes::<Path<String, '/'>, D>()
        .exact_size()
        .map(|pn| {
            let (p, n) = pn.unwrap();
            println!("{p} {n:?}");
            assert!(n.leaf);
            assert_eq!(p.chars().filter(|c| *c == p.separator()).count(), n.depth);
            p.into_inner()
        })
        .collect()
}

pub fn set_get<'de, M>(s: &mut M, path: &str, value: &'de [u8])
where
    M: TreeDeserialize<'de> + TreeSerialize + ?Sized,
{
    json::set(s, path, value).unwrap();
    let mut buf = vec![0; value.len()];
    let len = json::get(s, path, &mut buf[..]).unwrap();
    assert_eq!(&buf[..len], value);
}

pub fn transcode_tracked<N: Transcode + Default>(
    schema: &Schema,
    keys: impl IntoKeys,
) -> Result<(N, Node), DescendError<N::Error>> {
    let mut target = N::default();
    let mut tracked = keys.into_keys().track();
    match target.transcode(schema, &mut tracked) {
        Err(DescendError::Key(KeyError::TooShort)) => {}
        ret => {
            ret?;
        }
    }
    Ok((target, tracked.node()))
}
