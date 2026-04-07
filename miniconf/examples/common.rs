use miniconf::{Leaf, Tree, leaf};
use serde::{Deserialize, Serialize};

// Either/Inner/Settings are straight from README.md

/// Inner doc
#[derive(Deserialize, Serialize, Default, Clone, PartialEq, Eq, Tree)]
#[tree(meta(doc, typename))]
pub struct MyStruct {
    #[tree(meta(max = "10"))]
    pub a: i32,
    /// Outer doc
    pub b: u8,
}

/// Inner doc
#[derive(Deserialize, Serialize, Default, Clone, PartialEq, Eq, Tree)]
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

#[derive(Deserialize, Serialize, Default, Clone, PartialEq, Eq)]
pub struct Uni;

#[derive(Tree, Default, Clone, PartialEq, Eq)]
#[tree(meta(typename))]
pub struct Settings {
    pub foo: bool,
    #[tree(with=leaf)]
    pub enum_: MyEnum,
    #[tree(with=leaf)]
    pub struct_: MyStruct,
    #[tree(with=leaf)]
    pub array: [i32; 2],
    #[tree(with=leaf)]
    pub option: Option<i32>,
    #[tree(with=leaf)]
    pub uni: Uni,

    #[tree(skip)]
    #[allow(unused)]
    pub skipped: (),

    pub struct_tree: MyStruct,
    pub enum_tree: MyEnum,
    pub array_tree: [i32; 2],
    pub array_tree2: [MyStruct; 2],
    pub tuple_tree: (i32, MyStruct),
    pub option_tree: Option<i32>,
    pub option_tree2: Option<MyStruct>,
    pub array_option_tree: [Option<MyStruct>; 2],
    pub option_array: Option<Leaf<[i16; 2]>>,
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
        self.array_option_tree = core::array::repeat(Some(Default::default()));
        self.option_array = Some(Leaf([1, 2]));
    }
}
