use crate::{IntoKeys, Keys};
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
        Self::ZERO
    }
}

impl Packed {
    /// Number of bits in the representation including the marker bit
    pub const BITS: u32 = NonZeroUsize::BITS;

    /// The total number of bits this representation can store.
    pub const CAPACITY: u32 = Self::BITS - 1;

    /// The empty value
    pub const ZERO: Self = Self(
        // Slightly cumbersome to generate it with `const`
        NonZeroUsize::MIN
            .saturating_add(1)
            .saturating_pow(Self::CAPACITY),
    );

    /// Create a new `Packed` from a `usize`.
    ///
    /// The value must not be zero.
    #[inline]
    pub fn new(v: usize) -> Option<Self> {
        NonZeroUsize::new(v).map(Self)
    }

    /// Get the contained bit-packed indices representation as a `usize`.
    #[inline]
    pub const fn get(&self) -> usize {
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
    pub const fn len(&self) -> u32 {
        Self::CAPACITY - self.0.trailing_zeros()
    }

    /// Return the representation aligned to the LSB with the marker bit
    /// moved from the LSB to the MSB.
    #[inline]
    pub fn as_lsb(&self) -> NonZeroUsize {
        // Note(unwrap): we ensure there is at least the marker bit set
        NonZeroUsize::new(((self.0.get() >> 1) | (1 << Self::CAPACITY)) >> self.0.trailing_zeros())
            .unwrap()
    }

    /// Build a `Packed` from a LSB-aligned representation with the marker bit
    /// moved from the MSB the LSB.
    #[inline]
    pub fn from_lsb(value: NonZeroUsize) -> Self {
        // Note(unwrap): we ensure there is at least the marker bit set
        Self::new(((value.get() << 1) | 1) << value.leading_zeros()).unwrap()
    }

    /// Return the number of bits required to represent `num`.
    ///
    /// Ensures that at least one bit is allocated.
    #[inline]
    pub fn bits_for(num: usize) -> u32 {
        (Self::BITS - num.leading_zeros()).max(1)
    }

    /// Remove the given number of MSBs and return them.
    ///
    /// If the value does not contain sufficient bits
    /// it is left unchanged and `None` is returned.
    ///
    /// Note(panic): `bits` must not be zero.
    #[inline]
    pub fn pop_msb(&mut self, bits: u32) -> Option<usize> {
        let s = self.0.get();
        if let Some(v) = NonZeroUsize::new(s << bits) {
            self.0 = v;
            Some(s >> (Self::BITS - bits))
        } else {
            None
        }
    }

    /// Push the given number `bits` of `value` as new LSBs.
    ///
    /// Returns the remaining number of unused bits on success.
    #[inline]
    pub fn push_lsb(&mut self, bits: u32, value: usize) -> Option<u32> {
        debug_assert_eq!(value >> bits, 0);
        let mut s = self.0.get();
        let mut n = self.0.trailing_zeros();
        if bits <= n {
            s &= !(1 << n);
            n -= bits;
            s |= ((value << 1) | 1) << n;
            // Note(unwrap): we ensure there is at least the marker bit set
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
        self.pop_msb(Self::bits_for(len.saturating_sub(1)))
    }
}

impl IntoKeys for Packed {
    type IntoKeys = Self;
    #[inline]
    fn into_keys(self) -> Self::IntoKeys {
        self
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test() {
        let t = [1usize, 3, 4, 0, 1];
        let mut p = Packed::default();
        for t in t {
            let bits = Packed::bits_for(t);
            p.push_lsb(bits, t).unwrap();
        }
        for t in t {
            let bits = Packed::bits_for(t);
            assert_eq!(p.pop_msb(bits).unwrap(), t);
        }
    }
}
