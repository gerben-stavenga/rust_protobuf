// protocrap-codegen/src/static_gen.rs

use anyhow::Result;
use proc_macro2::{Literal, TokenStream};
use prost_reflect::{DynamicMessage, FieldDescriptor, MessageDescriptor, ReflectMessage, Value};
use quote::{format_ident, quote};

/// Generate static initializer for any proto message using runtime reflection
pub fn generate_static_dynamic(value: &DynamicMessage) -> Result<TokenStream> {
    let descriptor = value.descriptor();

    // Calculate has_bits
    let has_bits = calculate_has_bits(&value, &descriptor);
    let has_bits_tokens = generate_has_bits_array(&has_bits);

    // Generate field initializers
    let field_inits = generate_field_initializers(&value, &descriptor)?;

    // Parse type path
    let path_parts: Vec<_> = descriptor
        .full_name()
        .split(".")
        .map(|s| format_ident!("{}", s))
        .collect();

    Ok(quote! {
        {
            protocrap::#(#path_parts)::* ::ProtoType::from_static(
                #has_bits_tokens,
                #(#field_inits),*
            )
        }
    })
}

fn calculate_has_bits(value: &DynamicMessage, descriptor: &MessageDescriptor) -> Vec<u32> {
    let field_count = descriptor.fields().len();
    let word_count = (field_count + 31) / 32;
    let mut has_bits = vec![0u32; word_count.max(1)];

    for (idx, field) in descriptor.fields().enumerate() {
        if value.has_field(&field) {
            let word_idx = idx / 32;
            let bit_idx = idx % 32;
            has_bits[word_idx] |= 1u32 << bit_idx;
        }
    }

    has_bits
}

fn generate_has_bits_array(has_bits: &[u32]) -> TokenStream {
    let values: Vec<_> = has_bits
        .iter()
        .map(|&v| Literal::u32_unsuffixed(v))
        .collect();
    quote! { [#(#values),*] }
}

fn generate_field_initializers(
    value: &DynamicMessage,
    descriptor: &MessageDescriptor,
) -> Result<Vec<TokenStream>> {
    let mut inits = Vec::new();

    for field in descriptor.fields() {
        let init = if value.has_field(&field) {
            let field_value = value.get_field(&field);
            generate_field_value(&field_value)?.0
        } else {
            generate_default_value(&field)
        };

        inits.push(init);
    }

    Ok(inits)
}

fn generate_field_value(value: &Value) -> Result<(TokenStream, TokenStream)> {
    match value {
        Value::Bool(b) => Ok((quote! { #b }, quote! { bool })),
        Value::I32(v) | Value::EnumNumber(v) => {
            let lit = Literal::i32_unsuffixed(*v);
            Ok((quote! { #lit }, quote! { i32 }))
        }
        Value::I64(v) => {
            let lit = Literal::i64_unsuffixed(*v);
            Ok((quote! { #lit }, quote! { i64 }))
        }
        Value::U32(v) => {
            let lit = Literal::u32_unsuffixed(*v);
            Ok((quote! { #lit }, quote! { u32 }))
        }
        Value::U64(v) => {
            let lit = Literal::u64_unsuffixed(*v);
            Ok((quote! { #lit }, quote! { u64 }))
        }
        Value::F32(v) => {
            let lit = Literal::f32_unsuffixed(*v);
            Ok((quote! { #lit }, quote! { f32 }))
        }
        Value::F64(v) => {
            let lit = Literal::f64_unsuffixed(*v);
            Ok((quote! { #lit }, quote! { f64 }))
        }
        Value::String(s) => Ok((
            quote! {
                protocrap::containers::String::from_static(#s)
            },
            quote! { protocrap::containers::String },
        )),
        Value::Bytes(b) => {
            let bytes: Vec<_> = b.iter().map(|&byte| Literal::u8_unsuffixed(byte)).collect();
            Ok((
                quote! {
                    protocrap::containers::Bytes::from_static(&[#(#bytes),*])
                },
                quote! { protocrap::containers::Bytes },
            ))
        }
        Value::Message(msg) => {
            // Recursively generate nested message
            let init = generate_nested_message(msg)?;
            Ok((init, quote! { protocrap::base::Message }))
        }
        Value::List(list) => {
            let elements: Vec<_> = list
                .iter()
                .map(|v| generate_field_value(v))
                .collect::<Result<Vec<_>, _>>()?;

            let type_name = elements[0].1.clone();
            let elements: Vec<_> = elements.into_iter().map(|(init, _)| init).collect();
            let len = elements.len();
            Ok((
                quote! {
                    {
                        static ELEMENTS: [#type_name; #len] = [
                            #(#elements),*
                        ];
                        protocrap::containers::RepeatedField::from_static(&ELEMENTS)
                    }
                },
                quote! { protocrap::containers::RepeatedField<#type_name> },
            ))
        }
        Value::Map(_) => {
            // TODO: Handle maps
            panic!("Map fields not yet supported in static generation");
        }
    }
}

fn generate_nested_message(msg: &DynamicMessage) -> Result<TokenStream> {
    let nested_initializer = generate_static_dynamic(msg)?;
    // Parse type path
    let path_parts: Vec<_> = msg
        .descriptor()
        .full_name()
        .split(".")
        .map(|s| format_ident!("{}", s))
        .collect();

    Ok(quote! {
        {
            static PROTO_TYPE: protocrap::#(#path_parts)::* ::ProtoType = #nested_initializer;
            protocrap::base::Message::new(&PROTO_TYPE)
        }
    })
}

fn generate_default_value(field: &FieldDescriptor) -> TokenStream {
    use prost_reflect::{Cardinality, Kind};

    if field.cardinality() == Cardinality::Repeated {
        return quote! { protocrap::containers::RepeatedField::new() };
    }

    match field.kind() {
        Kind::String => quote! { protocrap::containers::String::new() },
        Kind::Bytes => quote! { protocrap::containers::Bytes::new() },
        Kind::Message(_) => quote! {
            protocrap::base::Message(core::ptr::null_mut())
        },
        Kind::Bool => quote! { false },
        Kind::Int32 | Kind::Sint32 | Kind::Sfixed32 => quote! { 0i32 },
        Kind::Int64 | Kind::Sint64 | Kind::Sfixed64 => quote! { 0i64 },
        Kind::Uint32 | Kind::Fixed32 => quote! { 0u32 },
        Kind::Uint64 | Kind::Fixed64 => quote! { 0u64 },
        Kind::Float => quote! { 0.0f32 },
        Kind::Double => quote! { 0.0f64 },
        Kind::Enum(_) => quote! { 0i32 },
    }
}
