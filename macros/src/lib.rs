//!
//! PyLy Helper Macros
//!

use quote::ToTokens;

fn s(ident: &str) -> syn::PathSegment {
    syn::PathSegment {
        ident: syn::Ident::new(ident, proc_macro::Span::call_site().into()),
        arguments: syn::PathArguments::None,
    }
}

///
/// Expose a Rust type to Python.
///
/// ### Examples
/// ```ignore
/// #[pyly::expose]
/// pub struct MyCoolType;
/// ```
///
#[proc_macro_attribute]
pub fn expose(
    _attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let mut item: syn::Item = syn::parse_macro_input!(item);
    if let syn::Item::Struct(syn::ItemStruct { attrs, .. })
    | syn::Item::Trait(syn::ItemTrait { attrs, .. }) = &mut item
    {
        attrs.push(syn::Attribute {
            pound_token: Default::default(),
            style: syn::AttrStyle::Outer,
            bracket_token: Default::default(),
            meta: syn::Meta::Path(syn::Path {
                leading_colon: None,
                segments: syn::punctuated::Punctuated::from_iter([s("__pyly"), s("__expose")]),
            }),
        });
    };

    item.into_token_stream().into()
}
