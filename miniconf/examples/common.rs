use miniconf::{Leaf, RangeLeaf, Tree};
use serde::{Deserialize, Serialize};

// Either/Inner/Settings are straight from README.md

/// Inner doc
#[derive(Deserialize, Serialize, Default, Tree)]
#[tree(doc, meta(name = "Inner"))]
pub struct Inner {
    #[tree(meta(max = "10"))]
    a: Leaf<i32>,
    /// Outer doc
    b: Leaf<i32>,
    /// Range limited
    c: RangeLeaf<u16, 0, 100>,
}

#[derive(Deserialize, Serialize, Default, Tree)]
#[tree(meta(name = "Either"))]
pub enum Either {
    #[default]
    Bad,
    Good,
    A(Leaf<i32>),
    B(Inner),
    C([Inner; 2]),
}

#[derive(Deserialize, Serialize, Default)]
pub struct Uni;

#[derive(Tree, Default)]
#[tree(meta(name = "Settings"))]
pub struct Settings {
    foo: Leaf<bool>,
    enum_: Leaf<Either>,
    struct_: Leaf<Inner>,
    array: Leaf<[i32; 2]>,
    option: Leaf<Option<i32>>,
    uni: Leaf<Uni>,

    #[tree(skip)]
    #[allow(unused)]
    skipped: (),

    struct_tree: Inner,
    enum_tree: Either,
    array_tree: [Leaf<i32>; 2],
    array_tree2: [Inner; 2],
    tuple_tree: (Leaf<i32>, Inner),
    option_tree: Option<Leaf<i32>>,
    option_tree2: Option<Inner>,
    array_option_tree: [Option<Inner>; 2],
}

impl Settings {
    /// Create a new enabled Settings
    pub fn new() -> Self {
        let mut s = Self::default();
        s.enable();
        s
    }

    /// Fill some of the Options
    pub fn enable(&mut self) {
        self.option_tree = Some(8.into());
        // self.enum_tree = Either::B(Default::default());
        self.enum_tree = Either::C(Default::default());
        self.option_tree2 = Some(Default::default());
        self.array_option_tree[1] = Some(Default::default());
    }
}
