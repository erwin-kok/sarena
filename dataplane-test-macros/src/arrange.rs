use proc_macro2::{Span, TokenStream};
use proc_macro2_diagnostics::{Diagnostic, SpanDiagnosticExt as _};
use syn::ItemFn;

use crate::common::{ProgramAttrs, ProgramMode, passthrough_tc, passthrough_xdp};

pub(crate) struct ArrangeProgram {
    item: ItemFn,
    mode: ProgramMode,
    name: String,
}

impl ArrangeProgram {
    pub(crate) fn parse(attrs: TokenStream, item: TokenStream) -> Result<Self, Diagnostic> {
        let item: ItemFn = syn::parse2(item)?;
        let ProgramAttrs { mode, name } =
            syn::parse2(attrs).map_err(|e| Span::call_site().error(e.to_string()))?;
        Ok(Self { item, mode, name })
    }

    pub(crate) fn expand(&self) -> TokenStream {
        match self.mode {
            ProgramMode::Tc => passthrough_tc("arrange", &self.name, &self.item),
            ProgramMode::Xdp => passthrough_xdp("arrange", &self.name, &self.item),
        }
    }
}

#[cfg(test)]
mod tests {
    use quote::quote;
    use syn::parse_quote;

    use super::*;

    #[test]
    fn test_tc_expand() {
        let prog = ArrangeProgram::parse(
            parse_quote! { tc, "my_test" },
            parse_quote! {
                fn prog(ctx: ::aya_ebpf::programs::TcContext) -> TestStatus {
                    TestStatus::Pass
                }
            },
        )
        .unwrap();
        let expanded = prog.expand();
        let expected = quote! {
            #[unsafe(no_mangle)]
            #[unsafe(link_section = "classifier")]
            fn __test_fw_arrange_my_test(ctx: *mut ::aya_ebpf::bindings::__sk_buff) -> i32 {
                let ctx = unsafe { ::core::ptr::NonNull::new_unchecked(ctx) };
                let tc_ctx = ::aya_ebpf::programs::TcContext::new(ctx);
                return prog(tc_ctx) as i32;

                fn prog(ctx: ::aya_ebpf::programs::TcContext) -> TestStatus {
                    TestStatus::Pass
                }
            }
        };
        assert_eq!(expected.to_string(), expanded.to_string());
    }

    #[test]
    fn test_xdp_expand_with_order() {
        let prog = ArrangeProgram::parse(
            parse_quote! { xdp, "firewall_test" },
            parse_quote! {
                fn check(ctx: ::aya_ebpf::programs::XdpContext) -> TestStatus {
                    TestStatus::Pass
                }
            },
        )
        .unwrap();
        assert_eq!(prog.name, "firewall_test");
        let expanded = prog.expand();
        let expected = quote! {
            #[unsafe(no_mangle)]
            #[unsafe(link_section = "xdp")]
            fn __test_fw_arrange_firewall_test(ctx: *mut ::aya_ebpf::bindings::xdp_md) -> u32 {
                return check(::aya_ebpf::programs::XdpContext::new(ctx)) as u32;

                fn check(ctx: ::aya_ebpf::programs::XdpContext) -> TestStatus {
                    TestStatus::Pass
                }
            }
        };
        assert_eq!(expected.to_string(), expanded.to_string());
    }
}
