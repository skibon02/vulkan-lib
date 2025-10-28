mod define_layout;

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, Data, Fields};

#[proc_macro_derive(CollectDrawStateUpdates)]
pub fn derive_collect_draw_state_updates(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let (updates, clear_updates) =
        if let Data::Struct(data) = &input.data {
            match &data.fields {
                Fields::Named(fields) => {
                    let update_calls: Vec<_> = fields.named.iter().map(|f| {
                        let field_name = &f.ident;
                        quote! {
                            self.#field_name.collect_updates()
                        }
                    }).collect();
                    let first_update_call = update_calls.first();
                    let rest_update_calls = update_calls.iter().skip(1);

                    let clear_calls: Vec<_> = fields.named.iter().map(|f| {
                        let field_name = &f.ident;
                        quote! {
                            self.#field_name.clear_updates();
                        }
                    }).collect();

                    (
                        quote! {
                            #first_update_call
                            #(.chain(#rest_update_calls))*
                        },
                        quote! {
                            #(#clear_calls)*
                        },
                    )
                }
                Fields::Unnamed(fields) => {
                    let mut update_calls = fields.unnamed.iter().enumerate().map(|(i, _)| {
                        let index = syn::Index::from(i);
                        quote! {
                            self.#index.collect_updates()
                        }
                    });
                    let first_update_call = update_calls.next();

                    let clear_calls = fields.unnamed.iter().enumerate().map(|(i, _)| {
                        let index = syn::Index::from(i);
                        quote! {
                            self.#index.clear_updates();
                        }
                    });

                    (
                        quote! {
                            #first_update_call
                            #(.chain(#update_calls))*
                        },
                        quote! {
                            #(#clear_calls)*
                        },
                    )
                }
                Fields::Unit => (
                    quote! { iter::empty() },
                    quote! {},
                ),
            }
        } else {
            (
                quote! { iter::empty() },
                quote! {},
            )
        };

    let expanded = quote! {
        impl render_core::collect_state::CollectDrawStateUpdates for #name {
            fn collect_updates(&self) -> impl Iterator<Item=render_core::GraphicsUpdateCmd> + '_ {
                #updates
            }

            fn clear_updates(&mut self) { 
                #clear_updates
            }
        }
    };

    TokenStream::from(expanded)
}



#[proc_macro]
pub fn define_layout(input: TokenStream) -> TokenStream {
    define_layout::define_layout(input)
}