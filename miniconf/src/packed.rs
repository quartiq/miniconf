use crate::{IntoKeys, Keys};
use core::{
    num::NonZeroUsize,
    ops::{Deref, DerefMut},
};

/// A bit-packed representation of `TreeKey` indices.
///
/// The value consists of a number of (from storage MSB to LSB):
///
/// * Zero or more groups of variable bit length, concatenated, each containing
///   the index at the given `TreeKey` level. The deepest level is last.
/// * A set bit to mark the end of the used bits.
/// * Zero or more cleared bits corresponding to unused index space.
///
/// [`Packed::EMPTY`] has the marker at the MSB.
/// During [`Packed::push_lsb()`] the values are inserted with their MSB
/// where the marker was and the marker moves toward the storage LSB.
/// During [`Packed::pop_msb()`] the values are removed with their MSB
/// aligned with the storage MSB and marker moves toward the storage MSB.
///
/// The representation is MSB aligned to make key `Ord` more natural and stable.
/// The `Packed` key `Ord` matches the ordering of node on a depth-first tree
/// traversal. New nodes can be added to the tree without changing the implicit
/// encoding as long no new bits need to be allocated. Under this condition
/// the mapping between indices and `Packed` representation is stable.
///
/// "Small numbers" in LSB-aligned representation can be obtained through
/// [`Packed::into_lsb()`]/[`Packed::from_lsb()`] but don't have the ordering
/// and stability properties.
///
/// `Packed` can be used to uniquely identify
/// nodes in a [`crate::TreeKey`] using only a very small amount of bits.
/// For many realistic `TreeKey`s a `u16` or even a `u8` is sufficient
/// to hold a `Packed` in LSB notation. Together with the `Postcard` trait
/// and `postcard` `serde` format, this then gives access to any node in a nested
/// heterogeneous `Tree` with just a `u16` or `u8` as compact key and `[u8]` as
/// compact value.
///
/// ```
/// # use miniconf::Packed;
///
/// let mut p = Packed::EMPTY;
/// let mut p_lsb = 0b1; // marker
/// for (bits, value) in [(2, 0b11), (1, 0b0), (0, 0b0), (3, 0b101)] {
///     p.push_lsb(bits, value).unwrap();
///     p_lsb <<= bits;
///     p_lsb |= value;
/// }
/// assert_eq!(p_lsb, 0b1_11_0__101);
/// assert_eq!(p, Packed::from_lsb(p_lsb.try_into().unwrap()));
/// assert_eq!(p.get(), 0b11_0__101_1 << (Packed::CAPACITY - p.len()));
/// ```
#[derive(Copy, Clone, Debug, PartialEq, PartialOrd, Eq, Ord, Hash)]
#[repr(transparent)]
pub struct Packed(
    // Could use generic_nonzero #120257
    NonZeroUsize,
);

impl Default for Packed {
    #[inline]
    fn default() -> Self {
        Self::EMPTY
    }
}

impl From<NonZeroUsize> for Packed {
    #[inline]
    fn from(value: NonZeroUsize) -> Self {
        Self(value)
    }
}

impl From<Packed> for NonZeroUsize {
    #[inline]
    fn from(value: Packed) -> Self {
        value.0
    }
}

impl Deref for Packed {
    type Target = NonZeroUsize;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Packed {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Packed {
    /// Number of bits in the representation including the marker bit
    pub const BITS: u32 = NonZeroUsize::BITS;

    /// The total number of bits this representation can store.
    pub const CAPACITY: u32 = Self::BITS - 1;

    /// The empty value
    pub const EMPTY: Self = Self(
        // Slightly cumbersome to generate it with `const`
        NonZeroUsize::MIN
            .saturating_add(1)
            .saturating_pow(Self::CAPACITY),
    );

    /// Create a new `Packed` from a `usize`.
    ///
    /// The value must not be zero.
    #[inline]
    pub const fn new(value: usize) -> Option<Self> {
        match NonZeroUsize::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// The value is empty.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        matches!(*self, Self::EMPTY)
    }

    /// Clear and discard all bits stored.
    #[inline]
    pub fn clear(&mut self) {
        *self = Self::EMPTY;
    }

    /// Number of bits stored.
    #[inline]
    pub const fn len(&self) -> u32 {
        Self::CAPACITY - self.0.trailing_zeros()
    }

    /// Return the representation aligned to the LSB with the marker bit
    /// moved from the LSB to the MSB.
    #[inline]
    pub fn into_lsb(&self) -> NonZeroUsize {
        // Note(unwrap): we ensure there is at least the marker bit set
        NonZeroUsize::new(((self.get() >> 1) | (1 << Self::CAPACITY)) >> self.trailing_zeros())
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
    /// # Args
    /// * `bits`: Number of bits to pop. `bits <= Self::CAPACITY`
    #[inline]
    pub fn pop_msb(&mut self, bits: u32) -> Option<usize> {
        let s = self.get();
        // Remove value from self
        if let Some(new) = Self::new(s << bits) {
            self.0 = new.0;
            // Extract value from old self
            // Done in two steps as bits + 1 can be Self::BITS which would wrap.
            Some((s >> (Self::CAPACITY - bits)) >> 1)
        } else {
            None
        }
    }

    /// Push the given number `bits` of `value` as new LSBs.
    ///
    /// Returns the remaining number of unused bits on success.
    ///
    /// # Args
    /// * `bits`: Number of bits to push. `bits <= Self::CAPACITY`
    /// * `value`: Value to push. `value >> bits == 0`
    #[inline]
    pub fn push_lsb(&mut self, bits: u32, value: usize) -> Option<u32> {
        debug_assert_eq!(value >> bits, 0);
        let mut n = self.trailing_zeros();
        let old_marker = 1 << n;
        if let Some(new_marker) = Self::new(old_marker >> bits) {
            n -= bits;
            // * Remove old marker
            // * Add value at offset n + 1
            //   This is done in two steps as n + 1 can be Self::BITS, which would wrap.
            // * Add new marker
            self.0 = (self.get() ^ old_marker) | ((value << n) << 1) | new_marker.0;
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
        // Check path encoding round trip.
        let t = [1usize, 3, 4, 0, 1];
        let mut p = Packed::EMPTY;
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
