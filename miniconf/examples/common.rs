use miniconf::{Leaf, Tree, leaf};
use serde::{Deserialize, Serialize};

// Either/Inner/Settings are straight from README.md

/// Inner doc
#[derive(Deserialize, Serialize, Default, Tree)]
#[tree(meta(doc, typename))]
pub struct MyStruct {
    #[tree(meta(max = "10"))]
    a: i32,
    /// Outer doc
    b: u8,
}

/// Inner doc
#[derive(Deserialize, Serialize, Default, Tree)]
#[tree(meta(doc, typename))]
pub enum MyEnum {
    #[default]
    Bad,
    Good,
    A(i32),
    /// Outer doc
    B(MyStruct),
    C([MyStruct; 2]),
}

#[derive(Deserialize, Serialize, Default)]
pub struct Uni;

#[derive(Tree, Default)]
#[tree(meta(typename))]
pub struct Settings {
    foo: bool,
    #[tree(with=leaf)]
    enum_: MyEnum,
    #[tree(with=leaf)]
    struct_: MyStruct,
    #[tree(with=leaf)]
    array: [i32; 2],
    #[tree(with=leaf)]
    option: Option<i32>,
    #[tree(with=leaf)]
    uni: Uni,

    #[tree(skip)]
    #[allow(unused)]
    skipped: (),

    struct_tree: MyStruct,
    enum_tree: MyEnum,
    array_tree: [i32; 2],
    array_tree2: [MyStruct; 2],
    tuple_tree: (i32, MyStruct),
    option_tree: Option<i32>,
    option_tree2: Option<MyStruct>,
    array_option_tree: [Option<MyStruct>; 2],
    option_array: Option<Leaf<[i16; 2]>>,
}

#[allow(unused)]
impl Settings {
    /// Create a new enabled Settings
    pub fn new() -> Self {
        let mut s = Self::default();
        s.enable();
        s
    }

    /// Fill some of the Options
    pub fn enable(&mut self) {
        self.option_tree = Some(8);
        self.enum_tree = MyEnum::C(Default::default());
        self.option_tree2 = Some(Default::default());
        self.array_option_tree[1] = Some(Default::default());
        self.option_array = Some(Leaf([1, 2]));
    }
}
