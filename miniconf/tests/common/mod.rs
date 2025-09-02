#![allow(unused)]

use miniconf::{
    json, DescendError, IntoKeys, KeyError, Keys, Packed, Path, Schema, Track, Transcode,
    TreeDeserialize, TreeSchema, TreeSerialize,
};

pub fn paths<const D: usize>(schema: &'static Schema) -> Vec<String> {
    assert!(schema
        .nodes::<Packed, D>()
        .exact_size()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .is_sorted());
    schema
        .nodes::<Track<Path<String, '/'>>, D>()
        .exact_size()
        .map(|pn| {
            let pn = pn.unwrap();
            println!("{pn:?}");
            // assert_eq!(p.chars().filter(|c| *c == p.separator()).count(), n);
            pn.inner.into_inner()
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
