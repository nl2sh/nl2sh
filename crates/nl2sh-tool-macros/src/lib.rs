//! Compile-time adapter generation; registration remains explicit.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    parse_macro_input, punctuated::Punctuated, Expr, ItemFn, Lit, LitStr, MetaNameValue, Token,
};

/// Generates a `Tool` adapter for an async prepare function.
///
/// Syntax: `#[tool(adapter = "AdapterName", name = "tool_name", description = "...",
/// category = "knowledge", risk = "read_only", requires = ["ima"], parallel_safe = true)]`.
/// The function must accept a context followed by one owned argument type and return a
/// `Result<PreparedToolCall>`. The macro does not register tools or grant permissions.
#[proc_macro_attribute]
pub fn tool(attributes: TokenStream, item: TokenStream) -> TokenStream {
    let attributes = parse_macro_input!(attributes with Punctuated::<MetaNameValue, Token![,]>::parse_terminated);
    let function = parse_macro_input!(item as ItemFn);
    match expand_tool(attributes, function) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand_tool(
    attributes: Punctuated<MetaNameValue, Token![,]>,
    function: ItemFn,
) -> syn::Result<proc_macro2::TokenStream> {
    if function.sig.asyncness.is_none() {
        return Err(syn::Error::new_spanned(
            function.sig.fn_token,
            "tool prepare function must be async",
        ));
    }
    let mut adapter = None;
    let mut name = None;
    let mut description = None;
    let mut category = None;
    let mut risk = None;
    let mut requires = None;
    let mut parallel_safe = None;
    for attribute in attributes {
        if attribute.path.is_ident("adapter") {
            set_once(
                &mut adapter,
                parse_ident(string_value(&attribute.value)?)?,
                &attribute,
            )?;
        } else if attribute.path.is_ident("name") {
            set_once(
                &mut name,
                string_value(&attribute.value)?.clone(),
                &attribute,
            )?;
        } else if attribute.path.is_ident("description") {
            set_once(
                &mut description,
                string_value(&attribute.value)?.clone(),
                &attribute,
            )?;
        } else if attribute.path.is_ident("category") {
            set_once(
                &mut category,
                parse_category(string_value(&attribute.value)?)?,
                &attribute,
            )?;
        } else if attribute.path.is_ident("risk") {
            set_once(
                &mut risk,
                parse_risk(string_value(&attribute.value)?)?,
                &attribute,
            )?;
        } else if attribute.path.is_ident("requires") {
            set_once(&mut requires, parse_requires(&attribute.value)?, &attribute)?;
        } else if attribute.path.is_ident("parallel_safe") {
            set_once(
                &mut parallel_safe,
                bool_value(&attribute.value)?,
                &attribute,
            )?;
        } else {
            return Err(syn::Error::new_spanned(
                attribute.path,
                "unknown tool attribute",
            ));
        }
    }
    let adapter =
        adapter.ok_or_else(|| syn::Error::new_spanned(&function.sig.ident, "missing adapter"))?;
    let name_value =
        name.ok_or_else(|| syn::Error::new_spanned(&function.sig.ident, "missing name"))?;
    let description = description
        .ok_or_else(|| syn::Error::new_spanned(&function.sig.ident, "missing description"))?;
    let category =
        category.ok_or_else(|| syn::Error::new_spanned(&function.sig.ident, "missing category"))?;
    let risk = risk.ok_or_else(|| syn::Error::new_spanned(&function.sig.ident, "missing risk"))?;
    let requires =
        requires.ok_or_else(|| syn::Error::new_spanned(&function.sig.ident, "missing requires"))?;
    let parallel_safe = parallel_safe
        .ok_or_else(|| syn::Error::new_spanned(&function.sig.ident, "missing parallel_safe"))?;
    let metadata = format_ident!("{}_METADATA", adapter.to_string().to_uppercase());
    if function.sig.inputs.len() != 2 {
        return Err(syn::Error::new_spanned(
            &function.sig.inputs,
            "tool prepare function needs context and arguments",
        ));
    }
    let args = match function.sig.inputs.iter().nth(1) {
        Some(syn::FnArg::Typed(argument)) => &argument.ty,
        _ => {
            return Err(syn::Error::new_spanned(
                &function.sig.inputs,
                "second parameter must be typed arguments",
            ))
        }
    };
    let name = &function.sig.ident;
    Ok(quote! {
        #function

        const #metadata: crate::tools::ToolMetadata = crate::tools::ToolMetadata {
            name: #name_value,
            description: #description,
            category: #category,
            risk: #risk,
            requires: &[#(#requires),*],
            parallel_safe: #parallel_safe,
        };

        pub(super) struct #adapter;

        #[async_trait::async_trait]
        impl crate::tools::Tool for #adapter {
            fn metadata(&self) -> &'static crate::tools::ToolMetadata {
                &#metadata
            }

            fn definition(&self) -> crate::llm::ToolDefinition {
                crate::tools::definition::<#args>(#metadata.name, #metadata.description)
            }

            async fn prepare(
                &self,
                ctx: &crate::tools::ToolContext<'_>,
                arguments: serde_json::Value,
            ) -> anyhow::Result<crate::tools::PreparedToolCall> {
                let args: #args = crate::tools::parse_args(#metadata.name, arguments)?;
                #name(ctx, args).await
            }
        }
    })
}

fn set_once<T>(slot: &mut Option<T>, value: T, attribute: &MetaNameValue) -> syn::Result<()> {
    if slot.is_some() {
        return Err(syn::Error::new_spanned(
            &attribute.path,
            "duplicate tool attribute",
        ));
    }
    *slot = Some(value);
    Ok(())
}

fn string_value(value: &Expr) -> syn::Result<&LitStr> {
    match value {
        Expr::Lit(syn::ExprLit {
            lit: Lit::Str(value),
            ..
        }) => Ok(value),
        _ => Err(syn::Error::new_spanned(value, "expected a string literal")),
    }
}

fn bool_value(value: &Expr) -> syn::Result<bool> {
    match value {
        Expr::Lit(syn::ExprLit {
            lit: Lit::Bool(value),
            ..
        }) => Ok(value.value),
        _ => Err(syn::Error::new_spanned(value, "expected a bool literal")),
    }
}

fn parse_category(value: &LitStr) -> syn::Result<proc_macro2::TokenStream> {
    match value.value().as_str() {
        "shell" => Ok(quote!(crate::tools::ToolCategory::Shell)),
        "file" => Ok(quote!(crate::tools::ToolCategory::File)),
        "knowledge" => Ok(quote!(crate::tools::ToolCategory::Knowledge)),
        _ => Err(syn::Error::new_spanned(value, "unsupported tool category")),
    }
}

fn parse_risk(value: &LitStr) -> syn::Result<proc_macro2::TokenStream> {
    match value.value().as_str() {
        "dynamic_shell" => Ok(quote!(crate::tools::ToolRisk::DynamicShell)),
        "read_only" => Ok(quote!(crate::tools::ToolRisk::ReadOnly)),
        "mutating" => Ok(quote!(crate::tools::ToolRisk::Mutating)),
        "dangerous" => Ok(quote!(crate::tools::ToolRisk::Dangerous)),
        "critical" => Ok(quote!(crate::tools::ToolRisk::Critical)),
        _ => Err(syn::Error::new_spanned(value, "unsupported tool risk")),
    }
}

fn parse_requires(value: &Expr) -> syn::Result<Vec<proc_macro2::TokenStream>> {
    let Expr::Array(array) = value else {
        return Err(syn::Error::new_spanned(value, "requires must be an array"));
    };
    array
        .elems
        .iter()
        .map(|item| match string_value(item)?.value().as_str() {
            "ima" => Ok(quote!(crate::tools::Capability::Ima)),
            _ => Err(syn::Error::new_spanned(item, "unsupported tool capability")),
        })
        .collect()
}

fn parse_ident(value: &LitStr) -> syn::Result<syn::Ident> {
    syn::parse_str(&value.value())
        .map_err(|_| syn::Error::new_spanned(value, "expected a Rust identifier"))
}

#[cfg(test)]
mod tests {
    use super::expand_tool;
    use syn::{parse::Parser, parse_quote, punctuated::Punctuated, ItemFn, MetaNameValue, Token};

    #[test]
    fn duplicate_risk_is_a_compile_error() -> syn::Result<()> {
        let attributes = Punctuated::<MetaNameValue, Token![,]>::parse_terminated.parse_str(
            "adapter = \"ExampleTool\", name = \"example\", description = \"example\", category = \"file\", risk = \"mutating\", risk = \"read_only\", requires = [], parallel_safe = false",
        )?;
        let function: ItemFn = parse_quote! {
            async fn prepare(_: &ToolContext<'_>, _: ExampleArgs) -> Result<PreparedToolCall> {
                Ok(PreparedToolCall::empty())
            }
        };
        assert!(expand_tool(attributes, function).is_err());
        Ok(())
    }
}
