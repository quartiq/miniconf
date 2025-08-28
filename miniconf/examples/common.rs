use miniconf::{Leaf, Tree};
use serde::{Deserialize, Serialize};

// Either/Inner/Settings are straight from README.md

/// Inner docs
#[derive(Deserialize, Serialize, Default, Tree)]
#[tree(meta(max = "innermeta"))]
pub struct Inner {
    #[tree(meta(min = "0", max = "10"))]
    a: Leaf<i32>,
    /// The `b` field
    b: Leaf<i32>,
}

#[derive(Deserialize, Serialize, Default, Tree)]
pub enum Either {
    #[default]
    Bad,
    Good,
    A(Leaf<i32>),
    B(Inner),
    C([Inner; 2]),
}

#[derive(Tree, Default)]
pub struct Settings {
    foo: Leaf<bool>,
    enum_: Leaf<Either>,
    struct_: Leaf<Inner>,
    array: Leaf<[i32; 2]>,
    option: Leaf<Option<i32>>,

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
