use miniconf::{TreeAny, TreeKey};

#[test]
fn any() {
    #[derive(TreeKey, TreeAny, Default)]
    struct S {
        foo: i32,
        #[tree(depth = 1)]
        bar: [i16; 2],
    }

    let mut s = S::default();
    let a = s.get_mut_by_key(["bar", "1"].into_iter()).unwrap();
    let r = a.downcast_mut::<i16>().unwrap();
    *r = 9;
    assert_eq!(s.bar[1], 9);
}
