use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, Data, DeriveInput, Fields, Type};

pub fn derive_attribute_enum_impl(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let struct_name = &input.ident;

    // Generate the enum name by replacing "Attributes" with "AttributeValue"
    let enum_name = format_ident!("{}Value",
        struct_name.to_string().trim_end_matches("Attributes"));

    // Extract fields
    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => panic!("AttributeEnum only supports structs with named fields"),
        },
        _ => panic!("AttributeEnum only supports structs"),
    };

    // Generate enum variants - one per field
    let enum_variants: Vec<_> = fields.iter().map(|field| {
        let field_name = field.ident.as_ref().unwrap();
        let field_type = &field.ty;

        // Convert field name to PascalCase for variant name
        let variant_name = to_pascal_case(&field_name.to_string());
        let variant_ident = format_ident!("{}", variant_name);

        quote! {
            #variant_ident(#field_type)
        }
    }).collect();

    // Generate match arms for applying individual field updates
    let apply_match_arms: Vec<_> = fields.iter().map(|field| {
        let field_name = field.ident.as_ref().unwrap();
        let variant_name = to_pascal_case(&field_name.to_string());
        let variant_ident = format_ident!("{}", variant_name);

        quote! {
            #enum_name::#variant_ident(value) => {
                self.#field_name = value;
            }
        }
    }).collect();

    // Generate the output
    let expanded = quote! {
        #[derive(Clone, Debug)]
        pub enum #enum_name {
            #(#enum_variants),*
        }

        impl #struct_name {
            pub fn apply(&mut self, value: #enum_name) {
                match value {
                    #(#apply_match_arms)*
                }
            }
        }

        impl From<Vec<#enum_name>> for #struct_name {
            fn from(values: Vec<#enum_name>) -> Self {
                let mut result = Self::default();
                for value in values {
                    result.apply(value);
                }
                result
            }
        }

        impl From<#enum_name> for #struct_name {
            fn from(value: #enum_name) -> Self {
                let mut result = Self::default();
                result.apply(value);
                result
            }
        }
    };

    TokenStream::from(expanded)
}

pub fn generate_parsed_attributes_impl(_input: TokenStream) -> TokenStream {
    let expanded = quote! {
        #[derive(Default, Debug, Clone)]
        pub struct ParsedAttributes {
            pub general: Option<GeneralAttributes>,
            pub text: Option<TextAttributes>,
            pub img: Option<ImgAttributes>,
            pub box_attr: Option<BoxAttributes>,
            pub row: Option<RowAttributes>,
            pub col: Option<ColAttributes>,
            pub stack: Option<StackAttributes>,
            pub self_child: Option<ChildAttributes>,
        }

        impl<A: smallvec::Array<Item = AttributeValue>> From<smallvec::SmallVec<A>> for ParsedAttributes {
            fn from(values: smallvec::SmallVec<A>) -> Self {
                let mut result = Self::default();

                for value in values {
                    match value {
                        AttributeValue::General(v) => {
                            result.general.get_or_insert_with(GeneralAttributes::default).apply(v);
                        }
                        AttributeValue::Text(v) => {
                            result.text.get_or_insert_with(TextAttributes::default).apply(v);
                        }
                        AttributeValue::Img(v) => {
                            result.img.get_or_insert_with(ImgAttributes::default).apply(v);
                        }
                        AttributeValue::Box(v) => {
                            result.box_attr.get_or_insert_with(BoxAttributes::default).apply(v);
                        }
                        AttributeValue::Row(v) => {
                            result.row.get_or_insert_with(RowAttributes::default).apply(v);
                        }
                        AttributeValue::Col(v) => {
                            result.col.get_or_insert_with(ColAttributes::default).apply(v);
                        }
                        AttributeValue::Stack(v) => {
                            result.stack.get_or_insert_with(StackAttributes::default).apply(v);
                        }
                        AttributeValue::RowChild(v, is_parent) => {
                            if is_parent {
                                result.row.get_or_insert_with(RowAttributes::default).children_default.apply(v);
                            } else {
                                result.self_child.get_or_insert_with(ChildAttributes::default).row.apply(v);
                            }
                        }
                        AttributeValue::ColChild(v, is_parent) => {
                            if is_parent {
                                result.col.get_or_insert_with(ColAttributes::default).children_default.apply(v);
                            } else {
                                result.self_child.get_or_insert_with(ChildAttributes::default).col.apply(v);
                            }
                        }
                        AttributeValue::StackChild(v, is_parent) => {
                            if is_parent {
                                result.stack.get_or_insert_with(StackAttributes::default).children_default.apply(v);
                            } else {
                                result.self_child.get_or_insert_with(ChildAttributes::default).stack.apply(v);
                            }
                        }
                    }
                }

                result
            }
        }
    };

    TokenStream::from(expanded)
}

fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect()
}
