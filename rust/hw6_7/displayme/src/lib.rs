use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, parse_macro_input};

/// TODO: implement the following procedural `#[derive(DisplayMe)]` macro
/// It should be usable only on structs. When used on enums (or unions), it should produce a compile
/// error.
///
/// The macro should generate code that will implement the `Display` trait for the struct. The
/// specific format of the display implementation is defined by tests in the `assignments` crate.
#[proc_macro_derive(DisplayMe)]
pub fn derive_display_me(stream: TokenStream) -> TokenStream {
    let input = parse_macro_input!(stream as DeriveInput);
    let name = &input.ident;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();

    let body = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Unit => {
                quote! {
                    ::std::write!(formatter, "struct {};", stringify!(#name))
                }
            }
            Fields::Unnamed(fields) => {
                let field_count = fields.unnamed.len();
                let field_output = (0..field_count).map(|number| {
                    let field_number = syn::Index::from(number);
                    if number + 1 == field_count {
                        quote! {
                            ::std::write!(
                                formatter,
                                "    {}: {}\n",
                                #number,
                                &self.#field_number
                            )?;
                        }
                    } else {
                        quote! {
                            ::std::write!(
                                formatter,
                                "    {}: {},\n",
                                #number,
                                &self.#field_number
                            )?;
                        }
                    }
                });

                quote! {
                    ::std::write!(formatter, "struct {} (\n", stringify!(#name))?;
                    #(#field_output)*
                    ::std::write!(formatter, ")")
                }
            }
            Fields::Named(fields) => {
                let field_count = fields.named.len();
                if field_count == 0 {
                    return quote! {
                        impl #impl_generics ::std::fmt::Display for #name #type_generics #where_clause {
                            fn fmt(
                                &self,
                                formatter: &mut ::std::fmt::Formatter<'_>,
                            ) -> ::std::fmt::Result {
                                ::std::write!(formatter, "struct {} {{}}", stringify!(#name))
                            }
                        }
                    }
                    .into();
                }

                let field_output = fields.named.iter().enumerate().map(|(number, field)| {
                    let field_name = field.ident.as_ref().unwrap();
                    let field_name_as_string = field_name.to_string();

                    if number + 1 == field_count {
                        quote! {
                            ::std::write!(
                                formatter,
                                "    {}: {}\n",
                                #field_name_as_string,
                                &self.#field_name
                            )?;
                        }
                    } else {
                        quote! {
                            ::std::write!(
                                formatter,
                                "    {}: {},\n",
                                #field_name_as_string,
                                &self.#field_name
                            )?;
                        }
                    }
                });

                quote! {
                    ::std::write!(formatter, "struct {} {{\n", stringify!(#name))?;
                    #(#field_output)*
                    ::std::write!(formatter, "}}")
                }
            }
        },
        Data::Enum(_) | Data::Union(_) => {
            return syn::Error::new_spanned(
                input,
                "DisplayMe can only be used on structs",
            )
            .to_compile_error()
            .into();
        }
    };

    quote! {
        impl #impl_generics ::std::fmt::Display for #name #type_generics #where_clause {
            fn fmt(
                &self,
                formatter: &mut ::std::fmt::Formatter<'_>,
            ) -> ::std::fmt::Result {
                #body
            }
        }
    }
    .into()
}
