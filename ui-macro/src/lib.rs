mod attribute_enum;

use proc_macro::TokenStream;

#[proc_macro_derive(AttributeEnum)]
pub fn derive_attribute_enum(input: TokenStream) -> TokenStream {
    attribute_enum::derive_attribute_enum_impl(input)
}

#[proc_macro]
pub fn generate_parsed_attributes(input: TokenStream) -> TokenStream {
    attribute_enum::generate_parsed_attributes_impl(input)
}
