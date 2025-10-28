use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, Data, DeriveInput, Fields, Ident, ItemStruct};

pub fn define_layout(input: TokenStream) -> TokenStream {
    // Parse the input TokenStream into a syntax tree
    let input = parse_macro_input!(input as ItemStruct);

    // Extract the struct name
    let struct_name = &input.ident;

    // Extract fields
    let fields = if let Fields::Named(fields) = &input.fields {
        &fields.named
    } else {
        panic!("Only named fields are supported in define_layout!");
    };

    // Generate MEMBER_META entries
    let mut member_meta_entries = Vec::new();
    let mut trait_methods = Vec::new();
    let mut trait_methods_defs = Vec::new();

    for (i, field) in fields.iter().enumerate() {
        let field_name = field.ident.as_ref().unwrap();
        let field_type = &field.ty;
        let glsl_type = match quote!(#field_type).to_string().as_str() {
            "vec4 < 0 >" => quote! { GlslTypeVariant::Vec4 },
            "vec2 < 0 >" => quote! { GlslTypeVariant::Vec2 },
            "uint < 0 >" => quote! { GlslTypeVariant::Uint },
            "float < 0 >" => quote! { GlslTypeVariant::Float },
            "int < 0 >" => quote! { GlslTypeVariant::Int },
            t => panic!("Unsupported type in define_layout: {}", t),
        };
        member_meta_entries.push(quote! {
            MemberMeta {
                name: stringify!(#field_name),
                range: offset_of!(#struct_name, #field_name)..offset_of!(#struct_name, #field_name) + size_of::<#field_type>(),
                ty: #glsl_type,
            }
        });

        let set_method_name = format_ident!("set_{}", field_name);
        let modify_method_name = format_ident!("modify_{}", field_name);

        let inner_type = quote! {
            <#field_type as render_core::GlslType> :: Inner
        };


        trait_methods_defs.push(quote! {
            fn #set_method_name(&mut self, value: #inner_type);
            fn #modify_method_name<F>(&mut self, f: F)
            where
                F: FnOnce(#inner_type) -> #inner_type;
        });

        trait_methods.push(quote! {
            fn #set_method_name(&mut self, value: #inner_type) {
                unsafe {
                    self.modify_field(|s| {
                        s.#field_name = value.into();
                        #struct_name::MEMBERS_META[#i].range.clone()
                    });
                }
            }

            fn #modify_method_name<F>(&mut self, f: F)
            where
                F: FnOnce(#inner_type) -> #inner_type,
            {
                unsafe {
                    self.modify_field(|s| {
                        s.#field_name = f(s.#field_name.into()).into();
                        #struct_name::MEMBERS_META[#i].range.clone()
                    });
                }
            }
        });
    }

    let pub_fields = fields.iter().map(|f| {
        let field_name = &f.ident;
        let field_type = &f.ty;
        quote! {
            pub #field_name: #field_type
        }
    });

    let trait_name = format_ident!("{}Ext", struct_name);
    // Generate the final struct implementation
    let expanded = quote! {
        #[derive(Copy, Clone)]
        #[repr(C, align(16))]
        pub struct #struct_name {
            #(#pub_fields),*
        }

        impl LayoutInfo for #struct_name {
            const MEMBERS_META: &'static [MemberMeta] = &[
                #(#member_meta_entries),*
            ];
        }

        pub trait #trait_name {
            #(#trait_methods_defs)*
        }

        impl #trait_name for StateUpdatesBytes<#struct_name> {
            #(#trait_methods)*
        }
    };

    TokenStream::from(expanded)
}
