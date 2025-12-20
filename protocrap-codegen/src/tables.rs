// protocrap-codegen/src/tables.rs

use crate::names::{rust_type_tokens, sanitize_field_name};
use anyhow::Result;
use proc_macro2::TokenStream;
use protocrap::google::protobuf::DescriptorProto::ProtoType as DescriptorProto;
use protocrap::google::protobuf::FieldDescriptorProto::ProtoType as FieldDescriptorProto;

use protocrap::reflection::{calculate_tag, is_message, is_repeated};
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
        .filter(|f| is_repeated(f))
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

        if is_message(field) {
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

    let num_entries = max_field_number as usize + 1;

    let mut aux_index_map = std::collections::HashMap::<i32, usize>::new();
    let aux_entries: Vec<_> = message
        .field()
        .iter()
        .filter(|f| is_message(f))
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

            let field_kind = field_kind_tokens(field);

            if is_message(field) {
                let aux_index = *aux_index_map.get(&field_number).unwrap();
                // Message field - offset points to aux entry
                quote! { protocrap::decoding::TableEntry::new(
                    #field_kind,
                    0,
                    core::mem::offset_of!(protocrap::decoding::TableWithEntries<#num_entries, #num_aux_entries>, 2) + #aux_index * core::mem::size_of::<protocrap::decoding::AuxTableEntry>(),
                ) }
            } else {
                let has_bit = has_bit_map.get(&field_number).copied().unwrap_or(0) as u32;

                quote! {
                    protocrap::decoding::TableEntry::new(
                        #field_kind,
                        #has_bit,
                        core::mem::offset_of!(ProtoType, #field_name)
                    )
                }
            }
        } else {
            quote! { protocrap::decoding::TableEntry(0) }
        }
    }).collect();

    Ok(quote! {
        pub static DECODING_TABLE: protocrap::decoding::TableWithEntries<#num_entries, #num_aux_entries> =
            protocrap::decoding::TableWithEntries(
                protocrap::decoding::Table {
                    num_entries: #num_entries as u16,
                    size: core::mem::size_of::<ProtoType>() as u16,
                    descriptor: &ProtoType::descriptor_proto(),
                },
                [#(#table_entries,)*],
                [#(#aux_entries,)*]
            );
    })
}

fn field_kind_tokens(field: &&FieldDescriptorProto) -> TokenStream {
    let kind = protocrap::reflection::field_kind_tokens(field);
    let ident = format_ident!("{kind:?}");
    quote! { protocrap::wire::FieldKind::#ident }
}
