use miniconf::{JsonCoreSlashOwned, Path, TreeKey};

pub fn paths<M, const Y: usize>() -> Vec<String>
where
    M: TreeKey<Y>,
{
    M::nodes::<Path<String, '/'>>()
        .exact_size()
        .map(|pn| {
            let (p, n) = pn.unwrap();
            assert!(n.is_leaf());
            assert_eq!(p.chars().filter(|c| *c == p.separator()).count(), n.depth());
            p.into_inner()
        })
        .collect()
}

pub fn set_get<M, const Y: usize>(s: &mut M, path: &str, value: &[u8])
where
    M: JsonCoreSlashOwned<Y>,
{
    s.set_json(path, value).unwrap();
    let mut buf = vec![0; value.len()];
    let len = s.get_json(path, &mut buf[..]).unwrap();
    assert_eq!(&buf[..len], value);
}
