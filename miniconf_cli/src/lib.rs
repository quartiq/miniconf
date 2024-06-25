use core::marker::PhantomData;

use miniconf::TreeKey;

#[derive(Debug, Default, PartialEq, PartialOrd)]
pub struct Menu<M, const Y: usize>
where
    M: TreeKey<Y>,
{
    _m: PhantomData<M>,
}

impl<M, const Y: usize> Menu<M, Y> where M: TreeKey<Y> {
    
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new() {
    }
}
