// protocrap-codegen/src/tables.rs

use crate::names::{rust_type_tokens, sanitize_field_name};
use anyhow::Result;
use proc_macro2::TokenStream;
use protocrap::google::protobuf::DescriptorProto::ProtoType as DescriptorProto;
use protocrap::google::protobuf::FieldDescriptorProto::ProtoType as FieldDescriptorProto;
use protocrap::google::protobuf::FieldDescriptorProto::{Type, Label};

use quote::{format_ident, quote};

pub fn generate_encoding_table(
    message: &DescriptorProto,
    has_bit_map: &std::collections::HashMap<i32, usize>,
) -> Result<TokenStream> {
    let field_count = message.field().len();

    let mut aux_index_map = std::collections::HashMap::<i32, usize>::new();
    let aux_entries: Vec<_> = message
        .field()
        .iter()
        .filter(|f| matches!(f.r#type().unwrap(), Type::TYPE_MESSAGE | Type::TYPE_GROUP))
        .map(|field| {
            let field_name = format_ident!("{}", sanitize_field_name(field.name()));
            let child_table = rust_type_tokens(field);
            let num_aux = aux_index_map.len();
            aux_index_map.insert(field.number(), num_aux);
            quote! {
                protocrap::encoding::AuxTableEntry {
                    offset: core::mem::offset_of!(ProtoType, #field_name),
                    child_table: &#child_table::ENCODING_TABLE.1,
                }
            }
        })
        .collect();
    let num_aux_entries = aux_entries.len();

    let entries: Vec<_> = message.field().iter().map(|field| {
        let field_name = format_ident!("{}", sanitize_field_name(field.name()));
        let has_bit = has_bit_map.get(&field.number()).copied().unwrap_or(0) as u8;
        let kind = field_kind_tokens(field);
        let encoded_tag = calculate_tag(field);

        if matches!(field.r#type().unwrap(), Type::TYPE_MESSAGE | Type::TYPE_GROUP) {
            let aux_index = *aux_index_map.get(&field.number()).unwrap();
            // Message field - offset points to aux entry
            quote! {
                protocrap::encoding::TableEntry {
                    has_bit: #has_bit,
                    kind: #kind,
                    offset: (
                        core::mem::offset_of!(protocrap::encoding::TableWithEntries<#field_count, #num_aux_entries>, 2) + 
                        #aux_index * core::mem::size_of::<protocrap::encoding::AuxTableEntry>() - 
                        core::mem::offset_of!(protocrap::encoding::TableWithEntries<#field_count, #num_aux_entries>, 1)
                    ) as u16,
                    encoded_tag: #encoded_tag,
                }
            }
        } else {
            quote! {
                protocrap::encoding::TableEntry {
                    has_bit: #has_bit,
                    kind: #kind,
                    offset: core::mem::offset_of!(ProtoType, #field_name) as u16,
                    encoded_tag: #encoded_tag,
                }
            }
        }
    }).collect();

    Ok(quote! {
        pub static ENCODING_TABLE: protocrap::encoding::TableWithEntries<#field_count, #num_aux_entries> =
            protocrap::encoding::TableWithEntries(
                    &ProtoType::descriptor_proto(),
                    [
                        #(#entries,)*
                    ],
                    [
                        #(#aux_entries,)*
                    ]
            );
    })
}

pub fn generate_decoding_table(
    message: &DescriptorProto,
    has_bit_map: &std::collections::HashMap<i32, usize>,
) -> Result<TokenStream> {
    // Calculate masked table parameters
    let max_field_number = message
        .field()
        .iter()
        .map(|f| f.number())
        .max()
        .unwrap_or(0);

    if max_field_number > 2047 {
        return Err(anyhow::anyhow!("Field numbers > 2047 not supported yet"));
    }

    let num_masked_bits = if max_field_number > 15 {
        log2_floor_non_zero(max_field_number as u32) + 2
    } else {
        4
    };

    let num_masked: usize = 1 << num_masked_bits;
    let mask = ((num_masked - 1) << 3) as u16;
    let num_entries = max_field_number as usize + 1;

    // Generate masked table
    let masked_entries: Vec<_> = (0..num_masked)
        .map(|i| {
            let field_number = (i & 15) | (((i >> 5) << 4) * ((i >> 4) & 1));

            let kind = message
                .field()
                .iter()
                .find(|f| f.number() == field_number as i32)
                .map(field_kind_tokens)
                .unwrap_or_else(|| quote! { protocrap::wire::FieldKind::Unknown });

            kind
        })
        .collect();

    let mut aux_index_map = std::collections::HashMap::<i32, usize>::new();
    let aux_entries: Vec<_> = message
        .field()
        .iter()
        .filter(|f| matches!(f.r#type().unwrap(), Type::TYPE_MESSAGE | Type::TYPE_GROUP))
        .map(|field| {
            let field_name = format_ident!("{}", sanitize_field_name(field.name()));
            let child_table = rust_type_tokens(field);
            let num_aux = aux_index_map.len();
            aux_index_map.insert(field.number(), num_aux);
            quote! {
                protocrap::decoding::AuxTableEntry {
                    offset: core::mem::offset_of!(ProtoType, #field_name) as u32,
                    child_table: &#child_table::DECODING_TABLE.0,
                }
            }
        })
        .collect();
    let num_aux_entries = aux_entries.len();

    // Generate entry table
    let table_entries: Vec<_> = (0..=max_field_number).map(|field_number| {
        if let Some(field) = message.field().iter().find(|f| f.number() == field_number as i32) {
            let field_name = format_ident!("{}", sanitize_field_name(field.name()));

            if matches!(field.r#type().unwrap(), Type::TYPE_MESSAGE | Type::TYPE_GROUP) {
                let aux_index = *aux_index_map.get(&field_number).unwrap();
                // Message field - offset points to aux entry
                quote! { protocrap::decoding::TableEntry(
                    (core::mem::offset_of!(protocrap::decoding::TableWithEntries<#num_masked, #num_entries, #num_aux_entries>, 3) + #aux_index * core::mem::size_of::<protocrap::decoding::AuxTableEntry>()) as u16) }
            } else {
                let has_bit = has_bit_map.get(&field_number).copied().unwrap_or(0);
                let has_bit_shifted = (has_bit << 10) as u16;

                quote! {
                    protocrap::decoding::TableEntry(
                        core::mem::offset_of!(ProtoType, #field_name) as u16 + #has_bit_shifted
                    )
                }
            }
        } else {
            quote! { protocrap::decoding::TableEntry(0) }
        }
    }).collect();

    Ok(quote! {
        pub static DECODING_TABLE: protocrap::decoding::TableWithEntries<#num_masked, #num_entries, #num_aux_entries> =
            protocrap::decoding::TableWithEntries(
                protocrap::decoding::Table {
                    mask: #mask,
                    size: core::mem::size_of::<ProtoType>() as u16,
                    descriptor: &ProtoType::descriptor_proto(),
                },
                [#(#masked_entries,)*],
                [#(#table_entries,)*],
                [#(#aux_entries,)*]
            );
    })
}

fn field_kind_tokens(field: &&FieldDescriptorProto) -> TokenStream {
    let base = match field.r#type().unwrap() {
        Type::TYPE_INT32 | Type::TYPE_UINT32 => "Varint32",
        Type::TYPE_INT64 | Type::TYPE_UINT64 => "Varint64",
        Type::TYPE_SINT32 => "Varint32Zigzag",
        Type::TYPE_SINT64 => "Varint64Zigzag",
        Type::TYPE_FIXED32 | Type::TYPE_SFIXED32 | Type::TYPE_FLOAT => "Fixed32",
        Type::TYPE_FIXED64 | Type::TYPE_SFIXED64 | Type::TYPE_DOUBLE => "Fixed64",
        Type::TYPE_BOOL => "Varint32",
        Type::TYPE_STRING | Type::TYPE_BYTES => "Bytes",
        Type::TYPE_MESSAGE => "Message",
        Type::TYPE_GROUP => "Group",
        Type::TYPE_ENUM => "Varint32",
    };

    let kind_name = if field.label().unwrap() == Label::LABEL_REPEATED {
        format!("Repeated{}", base)
    } else {
        base.to_string()
    };

    let ident = format_ident!("{}", kind_name);
    quote! { protocrap::wire::FieldKind::#ident }
}

fn calculate_tag(field: &FieldDescriptorProto) -> u32 {
    let wire_type = match field.r#type().unwrap() {
        Type::TYPE_INT32
        | Type::TYPE_INT64
        | Type::TYPE_UINT32
        | Type::TYPE_UINT64
        | Type::TYPE_SINT32
        | Type::TYPE_SINT64
        | Type::TYPE_BOOL
        | Type::TYPE_ENUM => 0,
        Type::TYPE_FIXED64 | Type::TYPE_SFIXED64 | Type::TYPE_DOUBLE => 1,
        Type::TYPE_STRING | Type::TYPE_BYTES | Type::TYPE_MESSAGE => 2,
        Type::TYPE_GROUP => 3,
        Type::TYPE_FIXED32 | Type::TYPE_SFIXED32 | Type::TYPE_FLOAT => 5,
    };

    (field.number() as u32) << 3 | wire_type
}

fn log2_floor_non_zero(n: u32) -> usize {
    if n == 0 {
        return 0;
    }
    31 - n.leading_zeros() as usize
}
