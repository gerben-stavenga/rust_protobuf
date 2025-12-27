// protocrap-codegen/src/static_gen.rs

use super::protocrap;
use anyhow::Result;
use proc_macro2::{Literal, TokenStream};
use protocrap::{
    google::protobuf::FieldDescriptorProto::{ProtoType as FieldDescriptorProto, Type},
    reflection::{DynamicMessageRef, Value, is_repeated, needs_has_bit},
};
use quote::{ToTokens, format_ident, quote};

fn full_name(name: &str) -> Vec<proc_macro2::Ident> {
    // HACK: Manually do name resolution to fully qualified type path
    let mut path_parts = Vec::new();
    path_parts.extend(
        ["google", "protobuf"]
            .iter()
            .map(|s| format_ident!("{}", s)),
    );
    if name == "ExtensionRange" {
        path_parts.push(format_ident!("DescriptorProto"));
    }
    if name == "ReservedRange" {
        path_parts.push(format_ident!("DescriptorProto"));
    }
    if name == "EditionDefault" {
        path_parts.push(format_ident!("FieldOptions"));
    }
    if name == "FeatureSupport" {
        path_parts.push(format_ident!("FieldOptions"));
    }
    if name == "Declaration" {
        path_parts.push(format_ident!("ExtensionRangeOptions"));
    }
    if name == "EnumReservedRange" {
        path_parts.push(format_ident!("EnumDescriptorProto"));
    }
    path_parts.push(format_ident!("{}", name));
    path_parts
}

/// Generate static initializer for any proto message using runtime reflection
pub(crate) fn generate_static_dynamic(value: &DynamicMessageRef) -> Result<TokenStream> {
    // Calculate has_bits
    let has_bits = calculate_has_bits(&value);
    let has_bits_tokens = generate_has_bits_array(&has_bits);

    // Generate field initializers
    let field_inits = generate_field_initializers(value)?;

    // Parse type path
    let path_parts: Vec<_> = full_name(value.descriptor().name());

    Ok(quote! {
        {
            protocrap::#(#path_parts)::* ::ProtoType::from_static(
                #has_bits_tokens,
                #(#field_inits),*
            )
        }
    })
}

fn calculate_has_bits(value: &DynamicMessageRef) -> Vec<u32> {
    let descriptor = value.descriptor();
    let num_has_bits = descriptor
        .field()
        .iter()
        .filter(|field| needs_has_bit(field))
        .count();
    let word_count = (num_has_bits + 31) / 32;
    let mut has_bits = vec![0u32; word_count];

    let mut has_bit_idx = 0;
    for &field in descriptor.field() {
        if !needs_has_bit(field) {
            continue;
        }
        if let Some(_) = value.get_field(field) {
            let word_idx = has_bit_idx / 32;
            let bit_idx = has_bit_idx % 32;
            has_bits[word_idx] |= 1u32 << bit_idx;
        }
        has_bit_idx += 1;
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

fn generate_field_initializers(value: &DynamicMessageRef) -> Result<Vec<TokenStream>> {
    let mut inits = Vec::new();

    let mut fields = Vec::from(value.descriptor().field());
    fields.sort_by_key(|f| f.number());

    for field in fields {
        let value = value.get_field(field);
        let init = if let Some(field_value) = value {
            generate_field_value(field_value)?.0
        } else {
            generate_default_value(&field)
        };

        inits.push(init);
    }

    Ok(inits)
}

fn generate_repeated_scalar<T: Copy + ToTokens>(
    values: &[T],
) -> Result<(TokenStream, TokenStream)> {
    let mut elements = Vec::new();
    let type_name = format_ident!("{}", std::any::type_name::<T>());

    for &value in values {
        let elem_init = quote! {
            #value
        };
        elements.push(elem_init);
    }

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

fn generate_field_value(value: Value) -> Result<(TokenStream, TokenStream)> {
    match value {
        Value::Bool(b) => Ok((quote! { #b }, quote! { bool })),
        Value::Int32(v) => {
            let lit = Literal::i32_unsuffixed(v);
            Ok((quote! { #lit }, quote! { i32 }))
        }
        Value::Int64(v) => {
            let lit = Literal::i64_unsuffixed(v);
            Ok((quote! { #lit }, quote! { i64 }))
        }
        Value::UInt32(v) => {
            let lit = Literal::u32_unsuffixed(v);
            Ok((quote! { #lit }, quote! { u32 }))
        }
        Value::UInt64(v) => {
            let lit = Literal::u64_unsuffixed(v);
            Ok((quote! { #lit }, quote! { u64 }))
        }
        Value::Float(v) => {
            let lit = Literal::f32_unsuffixed(v);
            Ok((quote! { #lit }, quote! { f32 }))
        }
        Value::Double(v) => {
            let lit = Literal::f64_unsuffixed(v);
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
            let init = generate_nested_message(&msg)?;
            Ok((init, quote! { protocrap::base::Message }))
        }
        Value::RepeatedBool(list) => generate_repeated_scalar(list),
        Value::RepeatedInt32(list) => generate_repeated_scalar(list),
        Value::RepeatedInt64(list) => generate_repeated_scalar(list),
        Value::RepeatedUInt32(list) => generate_repeated_scalar(list),
        Value::RepeatedUInt64(list) => generate_repeated_scalar(list),
        Value::RepeatedFloat(list) => generate_repeated_scalar(list),
        Value::RepeatedDouble(list) => generate_repeated_scalar(list),
        Value::RepeatedString(list) => {
            let mut elements = Vec::new();
            for s in list {
                let s_str = s.as_str();
                let elem_init = quote! {
                    protocrap::containers::String::from_static(#s_str)
                };
                elements.push(elem_init);
            }
            let len = elements.len();
            Ok((
                quote! {
                    {
                        static ELEMENTS: [protocrap::containers::String; #len] = [
                            #(#elements),*
                        ];
                        protocrap::containers::RepeatedField::from_static(&ELEMENTS)
                    }
                },
                quote! { protocrap::containers::RepeatedField<protocrap::containers::String> },
            ))
        }
        Value::RepeatedBytes(list) => {
            let mut elements = Vec::new();
            for b in list {
                let bytes: Vec<_> = b.iter().map(|&byte| Literal::u8_unsuffixed(byte)).collect();
                let elem_init = quote! {
                    protocrap::containers::Bytes::from_static(&[#(#bytes),*])
                };
                elements.push(elem_init);
            }
            let len = elements.len();
            Ok((
                quote! {
                    {
                        static ELEMENTS: [protocrap::containers::Bytes; #len] = [
                            #(#elements),*
                        ];
                        protocrap::containers::RepeatedField::from_static(&ELEMENTS)
                    }
                },
                quote! { protocrap::containers::RepeatedField<protocrap::containers::Bytes> },
            ))
        }
        Value::RepeatedMessage(list) => {
            let mut elements = Vec::new();
            for msg in list.iter() {
                let elem_init = generate_nested_message(&msg)?;
                elements.push(elem_init);
            }
            let len = elements.len();
            Ok((
                quote! {
                    {
                        static ELEMENTS: [protocrap::base::Message; #len] = [
                            #(#elements),*
                        ];
                        protocrap::containers::RepeatedField::from_static(&ELEMENTS)
                    }
                },
                quote! { protocrap::containers::RepeatedField<protocrap::base::Message> },
            ))
        }
    }
}

fn generate_nested_message(msg: &DynamicMessageRef) -> Result<TokenStream> {
    let nested_initializer = generate_static_dynamic(msg)?;
    // Parse type path
    let path_parts: Vec<_> = full_name(msg.descriptor().name());

    Ok(quote! {
        {
            static PROTO_TYPE: protocrap::#(#path_parts)::* ::ProtoType = #nested_initializer;
            protocrap::base::Message::new(&PROTO_TYPE)
        }
    })
}

fn generate_default_value(field: &FieldDescriptorProto) -> TokenStream {
    if is_repeated(field) {
        return quote! { protocrap::containers::RepeatedField::new() };
    }

    match field.r#type().unwrap() {
        Type::TYPE_STRING => quote! { protocrap::containers::String::new() },
        Type::TYPE_BYTES => quote! { protocrap::containers::Bytes::new() },
        Type::TYPE_MESSAGE | Type::TYPE_GROUP => quote! {
            protocrap::base::Message(core::ptr::null_mut())
        },
        Type::TYPE_BOOL => quote! { false },
        Type::TYPE_INT32 | Type::TYPE_SINT32 | Type::TYPE_SFIXED32 | Type::TYPE_ENUM => {
            quote! { 0i32 }
        }
        Type::TYPE_INT64 | Type::TYPE_SINT64 | Type::TYPE_SFIXED64 => quote! { 0i64 },
        Type::TYPE_UINT32 | Type::TYPE_FIXED32 => quote! { 0u32 },
        Type::TYPE_UINT64 | Type::TYPE_FIXED64 => quote! { 0u64 },
        Type::TYPE_FLOAT => quote! { 0.0f32 },
        Type::TYPE_DOUBLE => quote! { 0.0f64 },
    }
}
