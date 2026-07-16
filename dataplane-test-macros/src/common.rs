use std::fmt;

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{ItemFn, LitStr, Token, parse::ParseStream};

#[derive(Debug, Clone, Copy)]
pub(crate) enum ProgramMode {
    Tc,
    Xdp,
}

impl fmt::Display for ProgramMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tc => write!(f, "tc"),
            Self::Xdp => write!(f, "xdp"),
        }
    }
}

pub(crate) struct ProgramAttrs {
    pub(crate) mode: ProgramMode,
    pub(crate) name: String,
}

impl syn::parse::Parse for ProgramAttrs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let prog: syn::Ident = input.parse()?;
        let mode = match prog.to_string().as_str() {
            "tc" => ProgramMode::Tc,
            "xdp" => ProgramMode::Xdp,
            _ => return Err(syn::Error::new(prog.span(), "Expected `tc` or `xdp`")),
        };

        input.parse::<Token![,]>()?;
        let name: String = input.parse::<LitStr>()?.value();

        Ok(Self { mode, name })
    }
}

/// The `__test_fw_{kind}_{name}` naming convention the test runner's regex
/// (`dataplane-test-runner/src/ebpf_test_runner.rs`) discovers programs by.
pub(crate) fn outer_fn_ident(kind: &str, name: &str) -> syn::Ident {
    syn::Ident::new(&format!("__test_fw_{kind}_{name}"), Span::call_site())
}

pub(crate) fn passthrough_tc(kind: &str, name: &str, item: &ItemFn) -> TokenStream {
    let outer_fn = outer_fn_ident(kind, name);
    let inner_fn = &item.sig.ident;
    let vis = &item.vis;
    quote! {
        #[unsafe(no_mangle)]
        #[unsafe(link_section = "classifier")]
        #vis fn #outer_fn(ctx: *mut ::aya_ebpf::bindings::__sk_buff) -> i32 {
            let ctx = unsafe { ::core::ptr::NonNull::new_unchecked(ctx) };
            let tc_ctx = ::aya_ebpf::programs::TcContext::new(ctx);
            return #inner_fn(tc_ctx) as i32;

            #item
        }
    }
}

pub(crate) fn passthrough_xdp(kind: &str, name: &str, item: &ItemFn) -> TokenStream {
    let outer_fn = outer_fn_ident(kind, name);
    let inner_fn = &item.sig.ident;
    let vis = &item.vis;
    quote! {
        #[unsafe(no_mangle)]
        #[unsafe(link_section = "xdp")]
        #vis fn #outer_fn(ctx: *mut ::aya_ebpf::bindings::xdp_md) -> u32 {
            return #inner_fn(::aya_ebpf::programs::XdpContext::new(ctx)) as u32;

            #item
        }
    }
}
