use core::num::NonZero;

use crate::{DescendError, Internal, IntoKeys, Key, KeyError, Keys, Schema, Transcode};

/// A bit-packed representation of multiple indices.
///
/// Given known bit width of each index, the bits are
/// concatenated above a marker bit.
///
/// The value consists of (from storage MSB to LSB):
///
/// * Zero or more groups of variable bit length, concatenated, each containing
///   one index. The first is aligned with the storage MSB.
/// * A set bit to mark the end of the used bits.
/// * Zero or more cleared bits corresponding to unused index space.
///
/// [`Packed::EMPTY`] has the marker at the MSB.
/// During [`Packed::push_lsb()`] the indices are inserted with their MSB
/// where the marker was and the marker moves toward the storage LSB.
/// During [`Packed::pop_msb()`] the indices are removed with their MSB
/// aligned with the storage MSB and the remaining bits and the marker move
/// toward the storage MSB.
///
/// The representation is MSB aligned to make `PartialOrd`/`Ord` more natural and stable.
/// The `Packed` key `Ord` matches the ordering of nodes in a horizontal leaf tree
/// traversal. New nodes can be added/removed to the tree without changing the implicit
/// encoding (and ordering!) as long no new bits need to be allocated/deallocated (
/// as long as the number of child nodes of an internal node does not cross a
/// power-of-two boundary).
/// Under this condition the mapping between indices/paths and `Packed` representation
/// is stable even if child nodes are added/removed.
///
/// "Small numbers" in LSB-aligned representation can be obtained through
/// [`Packed::into_lsb()`]/[`Packed::from_lsb()`] but don't have the ordering
/// and stability properties.
///
/// `Packed` can be used to uniquely identify
/// nodes in a `TreeSchema` using only a very small amount of bits.
/// For many realistic `TreeSchema`s a `u16` or even a `u8` is sufficient
/// to hold a `Packed` in LSB notation. Together with the
/// `postcard` `serde` format, this then gives access to any node in a nested
/// heterogeneous `Tree` with just a `u16` or `u8` as compact key and `[u8]` as
/// compact value.
///
/// ```
/// use miniconf::Packed;
///
/// let mut p = Packed::EMPTY;
/// let mut p_lsb = 0b1; // marker
/// for (bits, value) in [(2, 0b11), (1, 0b0), (0, 0b0), (3, 0b101)] {
///     p.push_lsb(bits, value).unwrap();
///     p_lsb <<= bits;
///     p_lsb |= value;
/// }
/// assert_eq!(p_lsb, 0b1_11_0__101);
/// //                  ^ marker
/// assert_eq!(p, Packed::from_lsb(p_lsb.try_into().unwrap()));
/// assert_eq!(p.get(), 0b11_0__101_1 << (Packed::CAPACITY - p.len()));
/// //                              ^ marker
/// ```
#[derive(
    Copy, Clone, Debug, PartialEq, PartialOrd, Eq, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[repr(transparent)]
#[serde(transparent)]
pub struct Packed(pub NonZero<usize>);

impl Default for Packed {
    #[inline]
    fn default() -> Self {
        Self::EMPTY
    }
}

impl Packed {
    /// Number of bits in the representation including the marker bit
    pub const BITS: u32 = NonZero::<usize>::BITS;

    /// The total number of bits this representation can store.
    pub const CAPACITY: u32 = Self::BITS - 1;

    /// The empty value
    pub const EMPTY: Self = Self(
        // Slightly cumbersome to generate it with `const`
        NonZero::<usize>::MIN
            .saturating_add(1)
            .saturating_pow(Self::CAPACITY),
    );

    /// Create a new `Packed` from a `usize`.
    ///
    /// The value must not be zero.
    #[inline]
    pub const fn new(value: usize) -> Option<Self> {
        match NonZero::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Create a new `Packed` from LSB aligned `usize`
    ///
    /// The value must not be zero.
    #[inline]
    pub const fn new_from_lsb(value: usize) -> Option<Self> {
        match NonZero::new(value) {
            Some(value) => Some(Self::from_lsb(value)),
            None => None,
        }
    }

    /// The primitive value
    #[inline]
    pub const fn get(&self) -> usize {
        self.0.get()
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

    /// Number of bits that can be stored.
    #[inline]
    pub const fn capacity(&self) -> u32 {
        self.0.trailing_zeros()
    }

    /// Number of bits stored.
    #[inline]
    pub const fn len(&self) -> u32 {
        Self::CAPACITY - self.capacity()
    }

    /// Return the representation aligned to the LSB with the marker bit
    /// moved from the LSB to the MSB.
    #[inline]
    pub const fn into_lsb(self) -> NonZero<usize> {
        match NonZero::new(((self.0.get() >> 1) | (1 << Self::CAPACITY)) >> self.0.trailing_zeros())
        {
            Some(v) => v,
            // We ensure there is at least the marker bit set
            None => unreachable!(),
        }
    }

    /// Build a `Packed` from a LSB-aligned representation with the marker bit
    /// moved from the MSB the LSB.
    #[inline]
    pub const fn from_lsb(value: NonZero<usize>) -> Self {
        match Self::new(((value.get() << 1) | 1) << value.leading_zeros()) {
            Some(v) => v,
            // We ensure there is at least the marker bit set
            None => unreachable!(),
        }
    }

    /// Return the number of bits required to represent `num`.
    ///
    /// Ensures that at least one bit is allocated.
    #[inline]
    pub const fn bits_for(num: usize) -> u32 {
        match usize::BITS - num.leading_zeros() {
            0 => 1,
            v => v,
        }
    }

    /// Remove the given number of MSBs and return them.
    ///
    /// If the value does not contain sufficient bits
    /// it is left unchanged and `None` is returned.
    ///
    /// # Args
    /// * `bits`: Number of bits to pop. `bits <= Self::CAPACITY`
    pub fn pop_msb(&mut self, bits: u32) -> Option<usize> {
        let s = self.get();
        // Remove value from self
        Self::new(s << bits).map(|new| {
            *self = new;
            // Extract value from old self
            // Done in two steps as bits + 1 can be Self::BITS which would wrap.
            (s >> (Self::CAPACITY - bits)) >> 1
        })
    }

    /// Push the given number `bits` of `value` as new LSBs.
    ///
    /// Returns the remaining number of unused bits on success.
    ///
    /// # Args
    /// * `bits`: Number of bits to push. `bits <= Self::CAPACITY`
    /// * `value`: Value to push. `value >> bits == 0`
    pub fn push_lsb(&mut self, bits: u32, value: usize) -> Option<u32> {
        debug_assert_eq!(value >> bits, 0);
        let mut n = self.0.trailing_zeros();
        let old_marker = 1 << n;
        Self::new(old_marker >> bits).map(|new_marker| {
            n -= bits;
            // * Remove old marker
            // * Add value at offset n + 1
            //   Done in two steps as n + 1 can be Self::BITS, which would wrap.
            // * Add new marker
            self.0 = (self.get() ^ old_marker) | ((value << n) << 1) | new_marker.0;
            n
        })
    }
}

impl core::fmt::Display for Packed {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

impl Keys for Packed {
    #[inline]
    fn next(&mut self, internal: &Internal) -> Result<usize, KeyError> {
        let bits = Self::bits_for(internal.len().get() - 1);
        let index = self.pop_msb(bits).ok_or(KeyError::TooShort)?;
        index.find(internal).ok_or(KeyError::NotFound)
    }

    #[inline]
    fn finalize(&mut self) -> Result<(), KeyError> {
        if self.is_empty() {
            Ok(())
        } else {
            Err(KeyError::TooLong)
        }
    }
}

impl IntoKeys for Packed {
    type IntoKeys = Self;

    #[inline]
    fn into_keys(self) -> Self::IntoKeys {
        self
    }
}

impl Transcode for Packed {
    type Error = ();

    fn transcode(
        &mut self,
        schema: &Schema,
        keys: impl IntoKeys,
    ) -> Result<(), DescendError<Self::Error>> {
        schema.descend(keys.into_keys(), |_meta, idx_schema| {
            if let Some((index, internal)) = idx_schema {
                let bits = Packed::bits_for(internal.len().get() - 1);
                self.push_lsb(bits, index).ok_or(())?;
            }
            Ok(())
        })
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
