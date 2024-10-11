use miniconf::Tree;
use serde::{Deserialize, Serialize};

// Either/Inner/Settings are straight from README.md

#[derive(Deserialize, Serialize, Default, Tree)]
pub enum Either {
    #[default]
    Bad,
    Good,
    A(i32),
    B(#[tree(depth = 1)] Inner),
    C(#[tree(depth = 2)] [Inner; 2]),
}

#[derive(Deserialize, Serialize, Default, Tree)]
pub struct Inner {
    a: i32,
    b: i32,
}

#[derive(Tree, Default)]
pub struct Settings {
    foo: bool,
    enum_: Either,
    struct_: Inner,
    array: [i32; 2],
    option: Option<i32>,

    #[tree(skip)]
    #[allow(unused)]
    skipped: (),

    #[tree(depth = 1)]
    struct_tree: Inner,
    #[tree(depth = 3)]
    enum_tree: Either,
    #[tree(depth = 1)]
    array_tree: [i32; 2],
    #[tree(depth = 2)]
    array_tree2: [Inner; 2],

    #[tree(depth = 1)]
    option_tree: Option<i32>,
    #[tree(depth = 2)]
    option_tree2: Option<Inner>,
    #[tree(depth = 3)]
    array_option_tree: [Option<Inner>; 2],
}

impl Settings {
    /// Fill some of the Options
    pub fn enable(&mut self) {
        self.option_tree = Some(8);
        self.enum_tree = Either::B(Default::default());
        self.option_tree2 = Some(Default::default());
        self.array_option_tree[1] = Some(Default::default());
    }
}
