use super::{Error, Miniconf, MiniconfMetadata};

use core::fmt::Write;

pub struct MiniconfArray<T: Miniconf, const N: usize>(pub [T; N]);

impl<T: Miniconf, const N: usize> core::ops::Deref for MiniconfArray<T, N> {
    type Target = [T; N];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl<T: Miniconf, const N: usize> core::ops::DerefMut for MiniconfArray<T, N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T: Default + Miniconf + Copy, const N: usize> Default for MiniconfArray<T, N> {
    fn default() -> Self {
        Self([T::default(); N])
    }
}

impl<T: core::fmt::Debug + Miniconf, const N: usize> core::fmt::Debug for MiniconfArray<T, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

impl<T: PartialEq + Miniconf, const N: usize> PartialEq<[T; N]> for MiniconfArray<T, N> {
    fn eq(&self, other: &[T; N]) -> bool {
        self.0.eq(other)
    }
}

impl<T: PartialEq + Miniconf, const N: usize> PartialEq for MiniconfArray<T, N> {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq(&other.0)
    }
}

impl<T: Clone + Miniconf, const N: usize> Clone for MiniconfArray<T, N> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: Copy + Miniconf, const N: usize> Copy for MiniconfArray<T, N> {}

impl<T: Miniconf, const N: usize> Miniconf for MiniconfArray<T, N> {
    fn string_set(
        &mut self,
        mut topic_parts: core::iter::Peekable<core::str::Split<char>>,
        value: &[u8],
    ) -> Result<(), Error> {
        let next = topic_parts.next();
        if next.is_none() {
            return Err(Error::PathTooShort);
        }

        // Parse what should be the index value
        let i: usize = serde_json_core::from_str(next.unwrap())
            .or(Err(Error::BadIndex))?
            .0;

        if i >= self.0.len() {
            return Err(Error::BadIndex);
        }

        self.0[i].string_set(topic_parts, value)?;

        Ok(())
    }

    fn string_get(
        &self,
        mut topic_parts: core::iter::Peekable<core::str::Split<char>>,
        value: &mut [u8],
    ) -> Result<usize, Error> {
        let next = topic_parts.next();
        if next.is_none() {
            return Err(Error::PathTooShort);
        }

        // Parse what should be the index value
        let i: usize = serde_json_core::from_str(next.unwrap())
            .or(Err(Error::BadIndex))?
            .0;

        if i >= self.0.len() {
            return Err(Error::BadIndex);
        }

        self.0[i].string_get(topic_parts, value)
    }

    fn get_metadata(&self) -> MiniconfMetadata {
        // First, figure out how many digits the maximum index requires when printing.
        let mut index = N - 1;
        let mut num_digits = 0;

        while index > 0 {
            index /= 10;
            num_digits += 1;
        }

        let metadata = self.0[0].get_metadata();

        // If the sub-members have topic size, we also need to include an additional character for
        // the path separator. This is ommitted if the sub-members have no topic (e.g. fundamental
        // types, enums).
        if metadata.max_topic_size > 0 {
            MiniconfMetadata {
                max_topic_size: metadata.max_topic_size + num_digits + 1,
                max_depth: metadata.max_depth + 1,
            }
        } else {
            MiniconfMetadata {
                max_topic_size: num_digits,
                max_depth: metadata.max_depth + 1,
            }
        }
    }

    fn recurse_paths<const TS: usize>(
        &self,
        index: &mut [usize],
        topic: &mut heapless::String<TS>,
    ) -> Option<()> {
        let original_length = topic.len();

        if index.is_empty() {
            // Note: During expected execution paths using `into_iter()`, the size of the
            // index stack is checked in advance to make sure this condition doesn't occur.
            // However, it's possible to happen if the user manually calls `recurse_paths`.
            unreachable!("Index stack too small");
        }

        while index[0] < N {
            // Add the array index to the topic name.
            if topic.len() > 0 && topic.push('/').is_err() {
                // Note: During expected execution paths using `into_iter()`, the size of the
                // topic buffer is checked in advance to make sure this condition doesn't occur.
                // However, it's possible to happen if the user manually calls `recurse_paths`.
                unreachable!("Topic buffer too short");
            }

            if write!(topic, "{}", index[0]).is_err() {
                // Note: During expected execution paths using `into_iter()`, the size of the
                // topic buffer is checked in advance to make sure this condition doesn't occur.
                // However, it's possible to happen if the user manually calls `recurse_paths`.
                unreachable!("Topic buffer too short");
            }

            if self.0[index[0]]
                .recurse_paths(&mut index[1..], topic)
                .is_some()
            {
                return Some(());
            }

            // Strip off the previously prepended index, since we completed that element and need
            // to instead check the next one.
            topic.truncate(original_length);

            index[0] += 1;
            index[1..].iter_mut().for_each(|x| *x = 0);
        }

        None
    }
}

impl<T: crate::Serialize + crate::DeserializeOwned, const N: usize> Miniconf for [T; N] {
    fn string_set(
        &mut self,
        mut topic_parts: core::iter::Peekable<core::str::Split<char>>,
        value: &[u8],
    ) -> Result<(), Error> {
        let next = topic_parts.next();
        if next.is_none() {
            return Err(Error::PathTooShort);
        }

        // Parse what should be the index value
        let i: usize = serde_json_core::from_str(next.unwrap())
            .or(Err(Error::BadIndex))?
            .0;

        if i >= self.len() {
            return Err(Error::BadIndex);
        }

        if topic_parts.peek().is_some() {
            return Err(Error::PathTooLong);
        }

        self[i] = serde_json_core::from_slice(value)?.0;
        Ok(())
    }

    fn string_get(
        &self,
        mut topic_parts: core::iter::Peekable<core::str::Split<char>>,
        value: &mut [u8],
    ) -> Result<usize, Error> {
        let next = topic_parts.next();
        if next.is_none() {
            return Err(Error::PathTooShort);
        }

        // Parse what should be the index value
        let i: usize = serde_json_core::from_str(next.unwrap())
            .or(Err(Error::BadIndex))?
            .0;

        if i >= self.len() {
            return Err(Error::BadIndex);
        }

        if topic_parts.peek().is_some() {
            return Err(Error::PathTooLong);
        }

        serde_json_core::to_slice(&self[i], value).map_err(|_| Error::SerializationFailed)
    }

    fn get_metadata(&self) -> MiniconfMetadata {
        // First, figure out how many digits the maximum index requires when printing.
        let mut index = N - 1;
        let mut num_digits = 0;

        while index > 0 {
            index /= 10;
            num_digits += 1;
        }

        MiniconfMetadata {
            max_topic_size: num_digits,
            max_depth: 1,
        }
    }

    fn recurse_paths<const TS: usize>(
        &self,
        index: &mut [usize],
        topic: &mut heapless::String<TS>,
    ) -> Option<()> {
        if index.is_empty() {
            // Note: During expected execution paths using `into_iter()`, the size of the
            // index stack is checked in advance to make sure this condition doesn't occur.
            // However, it's possible to happen if the user manually calls `recurse_paths`.
            unreachable!("Index stack too small");
        }

        if index[0] < N {
            // Add the array index to the topic name.
            if topic.len() > 0 && topic.push('/').is_err() {
                // Note: During expected execution paths using `into_iter()`, the size of the
                // topic buffer is checked in advance to make sure this condition doesn't occur.
                // However, it's possible to happen if the user manually calls `recurse_paths`.
                unreachable!("Topic buffer too short");
            }

            if write!(topic, "{}", index[0]).is_err() {
                // Note: During expected execution paths using `into_iter()`, the size of the
                // topic buffer is checked in advance to make sure this condition doesn't occur.
                // However, it's possible to happen if the user manually calls `recurse_paths`.
                unreachable!("Topic buffer too short");
            }

            index[0] += 1;
            return Some(());
        }

        None
    }
}
