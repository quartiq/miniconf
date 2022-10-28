use proc_macro2::token_stream::IntoIter as TokenIter;
use proc_macro2::{TokenStream, TokenTree};
use std::str::FromStr;

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum MiniconfAttribute {
    Defer,
    Atomic,
}

impl FromStr for MiniconfAttribute {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        let attr = match s {
            "defer" => MiniconfAttribute::Defer,
            "atomic" => MiniconfAttribute::Atomic,
            other => return Err(format!("Unknown attribute: {other}")),
        };

        Ok(attr)
    }
}

pub struct AttributeParser {
    inner: TokenIter,
}

impl AttributeParser {
    pub fn new(stream: TokenStream) -> Self {
        Self {
            inner: stream.into_iter(),
        }
    }

    pub fn parse(&mut self) -> MiniconfAttribute {
        let first = self.inner.next().expect("A single keyword");

        match first {
            TokenTree::Group(group) => {
                let ident: syn::Ident = syn::parse2(group.stream()).expect("An identifier");

                MiniconfAttribute::from_str(&ident.to_string()).unwrap()
            }
            other => panic!("Unexpected tree: {:?}", other),
        }
    }
}
