#![cfg_attr(not(feature = "std"), no_std)]

use core::fmt::{self, Display, Write};
use core::marker::PhantomData;

use embedded_io_async::{Read as IoRead, Write as IoWrite};
use heapless::String;

use miniconf::{JsonCoreSlash, PathIter, Traversal, TreeKey};

#[derive(Debug, PartialEq)]
pub enum Error<I> {
    Fmt(fmt::Error),
    Traversal(miniconf::Traversal),
    Serialize(serde_json_core::ser::Error),
    Deserialize(serde_json_core::de::Error),
    Io(I),
}

impl<I> From<fmt::Error> for Error<I> {
    fn from(value: fmt::Error) -> Self {
        Self::Fmt(value)
    }
}

impl<I> From<Traversal> for Error<I> {
    fn from(value: Traversal) -> Self {
        Self::Traversal(value)
    }
}

impl<I> From<miniconf::Error<serde_json_core::ser::Error>> for Error<I> {
    fn from(value: miniconf::Error<serde_json_core::ser::Error>) -> Self {
        match value {
            miniconf::Error::Inner(_depth, e) => Self::Serialize(e),
            miniconf::Error::Traversal(e) => Self::Traversal(e),
            miniconf::Error::Finalization(_) => unreachable!(),
            _ => unimplemented!(),
        }
    }
}

impl<I> From<miniconf::Error<serde_json_core::de::Error>> for Error<I> {
    fn from(value: miniconf::Error<serde_json_core::de::Error>) -> Self {
        match value {
            miniconf::Error::Inner(_depth, e) => Self::Deserialize(e),
            miniconf::Error::Traversal(e) => Self::Traversal(e),
            miniconf::Error::Finalization(e) => Self::Deserialize(e),
            _ => unimplemented!(),
        }
    }
}

impl<I: Display> Display for Error<I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fmt(_) => write!(f, "Path buffer: too short"),
            Self::Traversal(e) => write!(f, "Traversal: {e}"),
            Self::Io(e) => write!(f, "IO: {e}"),
            Self::Deserialize(e) => write!(f, "Deserialization: {e}"),
            Self::Serialize(e) => write!(f, "Serialization: {e}"),
            _ => unreachable!(),
        }
    }
}

#[derive(Debug, PartialEq, PartialOrd)]
pub struct Menu<'a, M, const Y: usize, const S: usize>
where
    M: TreeKey<Y> + 'a,
{
    key: String<S>,
    _m: PhantomData<M>,
    separator: &'a str,
}

impl<'a, M, const Y: usize, const S: usize> Menu<'a, M, Y, S>
where
    M: TreeKey<Y>,
{
    pub fn new(separator: &'a str) -> Self {
        Self {
            key: String::new(),
            _m: PhantomData,
            separator,
        }
    }

    fn append(&self, path: Option<&str>) -> Result<String<S>, fmt::Error> {
        let mut key = self.key.clone();
        if let Some(path) = path {
            key.write_str(path)?;
        }
        Ok(key)
    }

    pub fn enter(&mut self, path: &str) -> Result<(), Error<()>> {
        let key = self.append(Some(path))?;
        M::indices(key.split(self.separator).skip(1))?;
        self.key = key;
        Ok(())
    }

    pub fn exit(&mut self) {
        if let Some(pos) = self.key.rfind(self.separator) {
            self.key.truncate(pos);
        }
    }

    pub fn list<const D: usize, E>(
        &self,
        path: Option<&str>,
    ) -> Result<impl Iterator<Item = Result<String<S>, fmt::Error>> + 'a, Error<E>> {
        let mut iter = PathIter::<M, Y, String<S>, D>::new(self.separator);
        let (idx, root) = M::indices(self.append(path)?.split(self.separator).skip(1))?;
        iter.root(idx[..root].iter().copied())?;
        Ok(iter)
    }

    pub fn get<E>(
        &self,
        instance: &M,
        path: Option<&str>,
        buf: &mut [u8],
    ) -> Result<usize, Error<E>>
    where
        M: for<'de> JsonCoreSlash<'de, Y>,
    {
        Ok(instance.get_json(&self.append(path)?, buf)?)
    }

    pub fn set<E>(
        &mut self,
        instance: &mut M,
        path: Option<&str>,
        buf: &[u8],
    ) -> Result<usize, Error<E>>
    where
        M: for<'de> JsonCoreSlash<'de, Y>,
    {
        Ok(instance.set_json(&self.append(path)?, buf)?)
    }

    pub async fn dump<W, const D: usize, const B: usize>(
        &self,
        instance: &M,
        path: Option<&str>,
        mut write: W,
    ) -> Result<usize, Error<W::Error>>
    where
        W: IoWrite,
        M: for<'de> JsonCoreSlash<'de, Y> + Default,
    {
        let mut buf = [0; B];
        let def = M::default();
        let mut len = 0;
        len += awrite(&mut write, self.key.as_bytes()).await?;
        len += awrite(&mut write, ">\n".as_bytes()).await?;
        for keys in self.list::<D, _>(path)? {
            let keys = keys?;
            len += awrite(&mut write, &keys.as_bytes()[self.key.len()..]).await?;
            len += awrite(&mut write, ": ".as_bytes()).await?;
            let ret = match instance.get_json(&keys, &mut buf) {
                Err(miniconf::Error::Traversal(Traversal::Absent(_depth))) => "(absent)".as_bytes(),
                ret => &buf[..ret?],
            };
            let check: u32 = yafnv::fnv1a(ret.iter().copied());
            len += awrite(&mut write, ret).await?;
            let ret = match def.get_json(&keys, &mut buf) {
                Err(miniconf::Error::Traversal(Traversal::Absent(_depth))) => "(absent)".as_bytes(),
                ret => &buf[..ret?],
            };
            if yafnv::fnv1a::<u32, _>(ret.iter().copied()) == check {
                len += awrite(&mut write, " (default)\n".as_bytes()).await?;
            } else {
                len += awrite(&mut write, " (default: ".as_bytes()).await?;
                len += awrite(&mut write, ret).await?;
                len += awrite(&mut write, ")\n".as_bytes()).await?;
            }
        }
        Ok(len)
    }
}

async fn awrite<W: IoWrite>(mut write: W, buf: &[u8]) -> Result<usize, Error<W::Error>> {
    write
        .write_all(buf)
        .await
        .map_err(Error::Io)
        .and(Ok(buf.len()))
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use miniconf::Tree;
    use tokio::io;

    use super::*;

    #[derive(Tree, Default)]
    struct Inner {
        e: i32,
    }

    #[derive(Tree, Default)]
    struct S {
        a: i32,
        #[tree(depth = 1)]
        b: [i32; 3],
        #[tree(depth = 1)]
        c: Option<i32>,
        #[tree(depth = 1)]
        d: Inner,
        #[tree(depth = 2)]
        f: [Inner; 2],
    }

    #[test]
    fn new() {
        let mut s = S::default();
        const D: usize = 16;
        const B: usize = 1024;
        let mut m = Menu::<S, 3, B>::new("/");
        for p in m.list::<D, ()>(None).unwrap() {
            println!("{}", p.unwrap());
        }
    }

    #[tokio::test]
    async fn dump() {
        let mut s = S::default();
        const D: usize = 16;
        const B: usize = 1024;
        let mut m = Menu::<S, 3, B>::new("/");
        let mut buf = [0u8; B];
        let len = m.dump::<_, D, B>(&s, None, &mut buf[..]).await.unwrap();
        println!("{}", core::str::from_utf8(&buf[..len]).unwrap());
        m.enter("/f").unwrap();
        let mut buf = [0u8; B];
        let len = m.dump::<_, D, B>(&s, None, &mut buf[..]).await.unwrap();
        println!("{}", core::str::from_utf8(&buf[..len]).unwrap());
    }
}
