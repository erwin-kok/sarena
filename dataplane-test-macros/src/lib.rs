use proc_macro::TokenStream;

mod act;
mod arrange;
mod assert;
mod common;

use crate::{act::ActProgram, arrange::ArrangeProgram, assert::AssertProgram};

#[proc_macro_attribute]
pub fn arrange(attrs: TokenStream, item: TokenStream) -> TokenStream {
    match ArrangeProgram::parse(attrs.into(), item.into()) {
        Ok(prog) => prog.expand(),
        Err(err) => err.emit_as_expr_tokens(),
    }
    .into()
}

#[proc_macro_attribute]
pub fn act(attrs: TokenStream, item: TokenStream) -> TokenStream {
    match ActProgram::parse(attrs.into(), item.into()) {
        Ok(prog) => prog.expand(),
        Err(err) => err.emit_as_expr_tokens(),
    }
    .into()
}

#[proc_macro_attribute]
pub fn assert(attrs: TokenStream, item: TokenStream) -> TokenStream {
    match AssertProgram::parse(attrs.into(), item.into()) {
        Ok(prog) => prog.expand(),
        Err(err) => err.emit_as_expr_tokens(),
    }
    .into()
}
