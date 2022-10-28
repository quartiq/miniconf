use proc_macro2::token_stream::IntoIter as TokenIter;
use proc_macro2::{TokenStream, TokenTree};

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum MiniconfAttribute {
    Defer,
}

pub struct AttributeParser {
    inner: TokenIter,
}

impl AttributeParser {
    pub fn new(stream: TokenStream) -> Self {
        Self {
            inner: stream.into_iter()
        }
    }

    pub fn parse(&mut self) -> MiniconfAttribute {
        let first = self.inner.next().expect("A single keyword");

        match first {
            TokenTree::Group(group) => {
                let ident: syn::Ident = syn::parse2(group.stream()).expect("An identifier");
                if ident != "defer" {
                    panic!("Unexpected miniconf attribute: {}", ident);
                }

                MiniconfAttribute::Defer
            }
            other => panic!("Unexpected tree: {:?}", other),
        }
    }
}
