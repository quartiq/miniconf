use miniconf::{json, Packed, Path, TreeDeserialize, TreeKey, TreeSerialize};

pub fn paths<M, const D: usize>() -> Vec<String>
where
    M: TreeKey,
{
    assert!(M::nodes::<_, D>()
        .exact_size()
        .collect::<Result<Vec<(Packed, _)>, _>>()
        .unwrap()
        .is_sorted());
    M::nodes::<Path<String, '/'>, D>()
        .exact_size()
        .map(|pn| {
            let (p, n) = pn.unwrap();
            assert!(n.is_leaf());
            assert_eq!(p.chars().filter(|c| *c == p.separator()).count(), n.depth());
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
