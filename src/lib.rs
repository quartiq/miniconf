use serde_json_core;

pub trait StringSet {
    fn string_set(&mut self, topic_parts:
            core::iter::Peekable<core::str::Split<char>>, value: &str) ->
            Result<(),()>;

}

macro_rules! derive_primative {
    ($x:ty) => {
        derive_single!($x);
        // This is needed until const generics is stabilized https://github.com/rust-lang/rust/issues/44580
        derive_array!($x, (1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32));
    }
}
macro_rules! derive_single {
    ($x:ty) => {
        impl StringSet for $x {
            fn string_set(&mut self, _topic_parts:
                core::iter::Peekable<core::str::Split<char>>, value: &str) ->
                Result<(),()> {
                *self = serde_json_core::from_str(value)
                    .map_err(|_|{()})?.0;
                Ok(())
            }
        }
    }
}

macro_rules! derive_array {
    ( $x:ty, ($($num:literal ),*) ) => {

            $(
                derive_single!([$x;$num]);
            )*
    };
}

// Implement trait for the primative types
derive_primative!(u8);
derive_primative!(u16);
derive_primative!(u32);
derive_primative!(u64);

derive_primative!(i8);
derive_primative!(i16);
derive_primative!(i32);
derive_primative!(i64);

derive_primative!(f32);
derive_primative!(f64);

derive_primative!(usize);
