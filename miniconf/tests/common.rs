#![allow(unused)]

use miniconf::{
    DescendError, IntoKeys, KeyError, Keys, Packed, Path, Schema, Transcode, TreeDeserialize,
    TreeSchema, TreeSerialize, json_core,
};

pub fn paths<T: TreeSchema, const D: usize>() -> Vec<String> {
    assert!(
        T::SCHEMA
            .nodes::<Packed, D>()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .is_sorted()
    );
    T::SCHEMA
        .nodes_with::<Path<String>, D>('/')
        .map(|pn| {
            let pn = pn.unwrap();
            println!("{pn:?}");
            pn.into_inner()
        })
        .collect()
}

pub fn set_get<'de, M>(s: &mut M, path: &str, value: &'de [u8])
where
    M: TreeDeserialize<'de> + TreeSerialize + ?Sized,
{
    json_core::set(s, path, value).unwrap();
    let mut buf = vec![0; value.len()];
    let len = json_core::get(s, path, &mut buf[..]).unwrap();
    assert_eq!(&buf[..len], value);
}
