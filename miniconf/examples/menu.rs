use core::fmt;
use core::marker::PhantomData;

use anyhow::{Context, Result};
use embedded_io::Write;
use embedded_io_async::Write as AWrite;
use postcard::{de_flavors::Slice as DeSlice, ser_flavors::Slice as SerSlice};
use tokio::io::AsyncBufReadExt;

use miniconf::{
    Indices, JsonCoreSlashOwned, Keys, Node, Packed, Path, PostcardOwned, Transcode, Traversal,
    TreeKey,
};

mod common;

/// Wrapper to support core::fmt::Write for embedded_io::Write
struct WriteWrap<T>(T);

impl<T: Write> fmt::Write for WriteWrap<T> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.0.write_all(s.as_bytes()).or(Err(fmt::Error))?;
        Ok(())
    }
}

async fn awrite<W: AWrite>(mut write: W, buf: &[u8]) -> Result<(), Error<W::Error>> {
    write.write_all(buf).await.map_err(Error::Io)
}

#[derive(Debug, PartialEq)]
pub enum Error<I> {
    Fmt(core::fmt::Error),
    Traversal(miniconf::Traversal),
    Serialize(usize, serde_json_core::ser::Error),
    Io(I),
}

impl<I> From<Traversal> for Error<I> {
    fn from(value: Traversal) -> Self {
        Self::Traversal(value)
    }
}

impl<I> From<miniconf::Error<serde_json_core::ser::Error>> for Error<I> {
    fn from(value: miniconf::Error<serde_json_core::ser::Error>) -> Self {
        match value {
            miniconf::Error::Inner(depth, e) => Self::Serialize(depth, e),
            miniconf::Error::Traversal(e) => Self::Traversal(e),
            miniconf::Error::Finalization(e) => Self::Serialize(0, e),
            _ => unimplemented!(),
        }
    }
}

impl<I> From<usize> for Error<I> {
    fn from(value: usize) -> Self {
        Traversal::TooLong(value).into()
    }
}

pub const SEPARATOR: char = '/';

#[derive(Debug, PartialEq, PartialOrd)]
pub struct Menu<M, const Y: usize, const D: usize = Y>
where
    M: TreeKey<Y> + ?Sized,
{
    key: Packed,
    _m: PhantomData<M>,
}

impl<M, const Y: usize> Default for Menu<M, Y>
where
    M: TreeKey<Y> + ?Sized,
{
    fn default() -> Self {
        Self::new(Packed::default())
    }
}

impl<M, const Y: usize> Menu<M, Y>
where
    M: TreeKey<Y> + ?Sized,
{
    pub fn new(key: Packed) -> Self {
        Self {
            key,
            _m: PhantomData,
        }
    }

    fn push(&self, path: &str) -> Result<(Self, Node), Traversal> {
        let (key, node) = M::transcode(self.key.chain(&Path::<_, SEPARATOR>::from(path)))?;
        Ok((Self::new(key), node))
    }

    fn pop(&self, levels: usize) -> Result<(Self, Node), Traversal> {
        let (idx, node) = M::transcode::<Indices<[_; Y]>, _>(self.key)?;
        if let Some(idx) = idx.get(
            ..node
                .depth()
                .checked_sub(levels)
                .ok_or(Traversal::TooLong(0))?,
        ) {
            let (key, node) = M::transcode(&Indices::from(idx))?;
            Ok((Self::new(key), node))
        } else {
            Err(Traversal::TooShort(node.depth()))
        }
    }

    pub fn enter(&mut self, path: &str) -> Result<Node, Traversal> {
        let (new, node) = self.push(path)?;
        *self = new;
        Ok(node)
    }

    pub fn exit(&mut self, levels: usize) -> Result<Node, Traversal> {
        let (new, node) = self.pop(levels)?;
        *self = new;
        Ok(node)
    }

    pub fn list<S: core::fmt::Write + Default>(
        &self,
    ) -> Result<impl Iterator<Item = Result<S, usize>>, Traversal> {
        Ok(M::nodes::<Path<S, SEPARATOR>>()
            .root(self.key)?
            .map(|pn| pn.map(|(p, _n)| p.into_inner())))
    }

    pub fn get(
        &self,
        instance: &M,
        buf: &mut [u8],
    ) -> Result<usize, miniconf::Error<serde_json_core::ser::Error>>
    where
        M: JsonCoreSlashOwned<Y>,
    {
        instance.get_json_by_key(self.key, buf)
    }

    pub fn set(
        &mut self,
        instance: &mut M,
        buf: &[u8],
    ) -> Result<usize, miniconf::Error<serde_json_core::de::Error>>
    where
        M: JsonCoreSlashOwned<Y>,
    {
        instance.set_json_by_key(self.key, buf)
    }

    pub fn reset(
        &mut self,
        instance: &mut M,
        buf: &mut [u8],
    ) -> Result<(), miniconf::Error<postcard::Error>>
    where
        M: PostcardOwned<Y> + Default,
    {
        let def = M::default();
        for keys in M::nodes::<Packed>().root(self.key)? {
            // Slight abuse of TooLong for "keys to long for packed"
            let (keys, node) =
                keys.map_err(|depth| miniconf::Error::Traversal(Traversal::TooLong(depth)))?;
            debug_assert!(node.is_leaf());
            let val = match def.get_postcard_by_key(keys, SerSlice::new(buf)) {
                Err(miniconf::Error::Traversal(Traversal::Absent(_))) => {
                    continue;
                }
                ret => ret?,
            };
            let _rest = match instance.set_postcard_by_key(keys, DeSlice::new(val)) {
                Err(miniconf::Error::Traversal(Traversal::Absent(_))) => {
                    continue;
                }
                ret => ret?,
            };
        }
        Ok(())
    }

    pub async fn dump<W>(
        &self,
        instance: &M,
        mut write: W,
        buf: &mut [u8],
    ) -> Result<(), Error<W::Error>>
    where
        W: AWrite,
        M: JsonCoreSlashOwned<Y> + Default,
    {
        let def = M::default();
        let bl = buf.len();
        let mut sl = &mut buf[..];
        Path::<_, SEPARATOR>::from(WriteWrap(&mut sl)).transcode::<M, Y, _>(self.key)?;
        let root_len = bl - sl.len();
        awrite(&mut write, &buf[..root_len]).await?;
        awrite(&mut write, ">\n".as_bytes()).await?;
        for keys in M::nodes::<Packed>().root(self.key)? {
            let (keys, node) = keys?;
            let (val, rest) = match instance.get_json_by_key(keys, &mut buf[..]) {
                Err(miniconf::Error::Traversal(Traversal::TooShort(_))) => {
                    debug_assert!(!node.is_leaf());
                    ("...\n".as_bytes(), &mut buf[..])
                }
                Err(miniconf::Error::Traversal(Traversal::Absent(_))) => {
                    continue;
                }
                ret => {
                    debug_assert!(node.is_leaf());
                    let (val, rest) = buf.split_at_mut(ret?) as _;
                    (val as _, rest)
                }
            };
            let check: u32 = yafnv::fnv1a(val);
            awrite(&mut write, "  ".as_bytes()).await?;
            let rl = rest.len();
            let mut sl = &mut rest[..];
            Path::<_, SEPARATOR>::from(WriteWrap(&mut sl)).transcode::<M, Y, _>(keys)?;
            let path_len = rl - sl.len();
            awrite(&mut write, &rest[root_len..path_len]).await?;
            awrite(&mut write, ": ".as_bytes()).await?;
            awrite(&mut write, val).await?;
            let def = match def.get_json_by_key(keys, &mut buf[..]) {
                Err(miniconf::Error::Traversal(Traversal::TooShort(_depth))) => {
                    debug_assert!(!node.is_leaf());
                    continue;
                }
                Err(miniconf::Error::Traversal(Traversal::Absent(_depth))) => "absent".as_bytes(),
                ret => &buf[..ret?],
            };
            if yafnv::fnv1a::<u32>(def) == check {
                awrite(&mut write, " [default]\n".as_bytes()).await?;
            } else {
                awrite(&mut write, " [default: ".as_bytes()).await?;
                awrite(&mut write, def).await?;
                awrite(&mut write, "]\n".as_bytes()).await?;
            }
        }
        Ok(())
    }

    async fn handle_cmd(
        &mut self,
        line: &str,
        mut stdout: impl AWrite,
        buf: &mut [u8],
        instance: &mut M,
    ) -> anyhow::Result<String>
    where
        M: JsonCoreSlashOwned<Y> + Default,
    {
        let mut args = line.splitn(2, ' ');
        Ok(match args.next().context("command")? {
            "enter" => {
                let path = args.next().context("path")?;
                self.enter(path)
                    .map_err(anyhow::Error::msg)
                    .map(|node| format!("{node:?}"))?
            }
            "exit" => {
                let levels = args.next().and_then(|v| str::parse(v).ok()).unwrap_or(1);
                self.exit(levels)
                    .map_err(anyhow::Error::msg)
                    .map(|node| format!("{node:?}"))?
            }
            "get" => self
                .get(instance, &mut buf[..])
                .map_err(anyhow::Error::msg)
                .map(|len| String::from_utf8(buf[..len].to_owned()).unwrap())?,
            "set" => self
                .set(instance, args.next().context("value")?.as_bytes())
                .map_err(anyhow::Error::msg)
                .and(Ok("".to_owned()))?,
            "dump" => self
                .dump(instance, &mut stdout, buf)
                .await
                .map_err(|err| anyhow::Error::msg(format!("{err:?}")))
                .and(Ok("".to_owned()))?,
            "reset" => self
                .reset(instance, buf)
                .map_err(anyhow::Error::msg)
                .and(Ok("".to_owned()))?,
            cmd => format!("no such command: {cmd}"),
        })
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let mut buf = vec![0; 1024];
    let mut s = common::Settings::default();
    s.enable();

    let mut stdout = embedded_io_adapters::tokio_1::FromTokio::new(tokio::io::stdout());
    let mut stdin = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    let mut menu = Menu::default();

    while let Some(line) = stdin.next_line().await? {
        let ret = menu
            .handle_cmd(line.as_str(), &mut stdout, &mut buf[..], &mut s)
            .await;
        awrite(&mut stdout, format!("{:?}", ret).as_bytes())
            .await
            .unwrap();
        awrite(&mut stdout, "\n".as_bytes()).await.unwrap();
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn test() {
        let mut buf = vec![0; 1024];
        let mut s = common::Settings::default();
        s.enable();

        let mut stdout = embedded_io_adapters::tokio_1::FromTokio::new(tokio::io::stdout());
        let mut menu = Menu::default();

        menu.enter("/option_tree2").unwrap();
        menu.enter("/b").unwrap();
        menu.set(&mut s, b"1234").unwrap();
        menu.exit(2).unwrap();
        menu.push("/array_option_tree/1/a")
            .unwrap()
            .0
            .set(&mut s, b"9")
            .unwrap();
        let paths: Vec<heapless::String<128>> = menu.list().unwrap().map(Result::unwrap).collect();
        stdout
            .write_all(format!("{:?}\n", paths).as_bytes())
            .await
            .unwrap();
        menu.dump(&s, &mut stdout, &mut buf).await.unwrap();
        menu.enter("/struct_tree").unwrap();
        menu.dump(&s, &mut stdout, &mut buf).await.unwrap();
        menu.exit(1).unwrap();
        menu.push("/struct_")
            .unwrap()
            .0
            .reset(&mut s, &mut buf)
            .unwrap();
        menu.push("/option_tree2")
            .unwrap()
            .0
            .reset(&mut s, &mut buf)
            .unwrap();
        menu.dump(&s, &mut stdout, &mut buf).await.unwrap();
    }
}
