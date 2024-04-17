use crate::{IntoKeys, Keys, TreeKey};
use core::num::NonZeroUsize;

/// A bit-packed representation of `TreeKey` indices.
///
/// The value consists of a number of (from MSB to LSB):
///
/// * Zero or more groups of variable bit length, concatenated, each containing
///   the index at the given `TreeKey` level. The deepest level is last.
/// * A set marker bit
/// * Zero or more cleared bits corresponding to unused index space.
#[derive(Copy, Clone, Debug, PartialEq, PartialOrd, Eq, Ord)]
#[repr(transparent)]
pub struct Packed(NonZeroUsize);

impl Default for Packed {
    #[inline]
    fn default() -> Self {
        Self::new(1 << (usize::BITS - 1)).unwrap()
    }
}

impl Packed {
    /// Create a new `Packed` from a `usize`.
    ///
    /// The value must not be zero.
    #[inline]
    pub fn new(v: usize) -> Option<Self> {
        NonZeroUsize::new(v).map(Self)
    }

    /// Get the contained bit-packed indices representation as a `usize`.
    #[inline]
    pub fn get(&self) -> usize {
        self.0.get()
    }

    /// The value is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Clear and discard all bits pushed.
    #[inline]
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Number of bits set (previously pushed and now available for `pop()`).
    #[inline]
    pub fn len(&self) -> u32 {
        usize::BITS - 1 - self.0.get().trailing_zeros()
    }

    /// Remove the given number of MSBs and return them.
    ///
    /// If the value does not contain sufficient bits, it is cleared and `None` is returned.
    #[inline]
    pub fn pop_msb(&mut self, bits: u32) -> Option<usize> {
        let s = self.0.get();
        if let Some(v) = NonZeroUsize::new(s << bits) {
            self.0 = v;
            Some(s >> (usize::BITS - bits))
        } else {
            self.clear();
            None
        }
    }

    /// Push the given number `bits` of `value` as new LSBs.
    ///
    /// Returns the remaining unused number of bits on success.
    #[inline]
    pub fn push_lsb(&mut self, bits: u32, value: usize) -> Option<u32> {
        debug_assert_eq!(value >> bits, 0);
        let mut s = self.0.get();
        let mut n = s.trailing_zeros();
        if bits <= n {
            s &= !(1 << n);
            n -= bits;
            s |= ((value << 1) | 1) << n;
            self.0 = NonZeroUsize::new(s).unwrap();
            Some(n)
        } else {
            None
        }
    }
}

impl Keys for Packed {
    type Item = usize;
    #[inline]
    fn next(&mut self, len: usize) -> Option<Self::Item> {
        self.pop_msb(usize::BITS - (len - 1).leading_zeros())
    }
}

impl IntoKeys for Packed {
    type IntoKeys = Self;
    #[inline]
    fn into_keys(self) -> Self::IntoKeys {
        self
    }
}
