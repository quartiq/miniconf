use miniconf::Tree;

#[derive(Tree)]
pub enum EnumUninhab {}

#[derive(Tree)]
pub enum EnumEmpty {#[tree(skip)] V}

#[derive(Tree)]
pub struct StructUnit;

#[derive(Tree)]
pub struct StructUnitTuple ();

#[derive(Tree)]
pub struct StructEmptyTuple (#[tree(skip)] ());

#[derive(Tree)]
pub struct StructEmpty {#[tree(skip)] _f: ()}

fn main() {}
