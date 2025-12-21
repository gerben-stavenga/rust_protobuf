use core::num;

use crate::names::{rust_type_tokens, sanitize_field_name};
use anyhow::Result;
use proc_macro2::TokenStream;
use super::protocrap;
use protocrap::google::protobuf::DescriptorProto::ProtoType as DescriptorProto;
use protocrap::google::protobuf::FieldDescriptorProto::ProtoType as FieldDescriptorProto;

use protocrap::reflection::{calculate_tag, is_message};
use quote::{format_ident, quote};

fn generate_aux_entries(
    message: &DescriptorProto,
    aux_index_map: &mut std::collections::HashMap<i32, usize>,
) -> Result<Vec<TokenStream>> {
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
                protocrap::tables::AuxTableEntry {
                    offset: core::mem::offset_of!(ProtoType, #field_name) as u32,
                    child_table: &#child_table::TABLE.table,
                }
            }
        })
        .collect();
    Ok(aux_entries)
}

fn generate_encoding_entries(
    message: &DescriptorProto,
    has_bit_map: &std::collections::HashMap<i32, usize>,
    aux_index_map: &std::collections::HashMap<i32, usize>,
) -> Result<Vec<TokenStream>> {
    let num_encode_entries = message.field().len();
    let num_aux_entries = aux_index_map.len();

    let max_field_number = message
        .field()
        .iter()
        .map(|f| f.number())
        .max()
        .unwrap_or(0);

    if max_field_number > 2047 {
        return Err(anyhow::anyhow!("Field numbers > 2047 not supported yet"));
    }

    let num_decode_entries = max_field_number as usize + 1;

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
                        core::mem::offset_of!(protocrap::tables::TableWithEntries<#num_encode_entries, #num_decode_entries, #num_aux_entries>, aux_entries) +
                        #aux_index * core::mem::size_of::<protocrap::tables::AuxTableEntry>() - 
                        core::mem::offset_of!(protocrap::tables::TableWithEntries<#num_encode_entries, #num_decode_entries, #num_aux_entries>, table)
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

    Ok(entries)
}

fn generate_decoding_table(
    message: &DescriptorProto,
    has_bit_map: &std::collections::HashMap<i32, usize>,
    aux_index_map: &std::collections::HashMap<i32, usize>,
) -> Result<Vec<TokenStream>> {
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

    let num_decode_entries = max_field_number as usize + 1;

    let num_encode_entries = message.field().len();

    let num_aux_entries = aux_index_map.len();

    // Generate entry table
    let entries: Vec<_> = (0..=max_field_number).map(|field_number| {
        if let Some(field) = message.field().iter().find(|f| f.number() == field_number as i32) {
            let field_name = format_ident!("{}", sanitize_field_name(field.name()));

            let field_kind = field_kind_tokens(field);

            if is_message(field) {
                let aux_index = *aux_index_map.get(&field_number).unwrap();
                // Message field - offset points to aux entry
                quote! { protocrap::decoding::TableEntry::new(
                    #field_kind,
                    0,
                    core::mem::offset_of!(protocrap::tables::TableWithEntries<#num_encode_entries, #num_decode_entries, #num_aux_entries>, aux_entries) + 
                    #aux_index * core::mem::size_of::<protocrap::tables::AuxTableEntry>() - 
                    core::mem::offset_of!(protocrap::tables::TableWithEntries<#num_encode_entries, #num_decode_entries, #num_aux_entries>, table)
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

    Ok(entries)
}

pub fn generate_table(
    message: &DescriptorProto,
    has_bit_map: &std::collections::HashMap<i32, usize>,
) -> Result<TokenStream> {
    let mut aux_index_map = std::collections::HashMap::<i32, usize>::new();
    let aux_entries = generate_aux_entries(message, &mut aux_index_map)?;

    let encoding_entries = generate_encoding_entries(message, has_bit_map, &aux_index_map)?;
    let decoding_entries = generate_decoding_table(message, has_bit_map, &aux_index_map)?;

    let num_encode_entries = encoding_entries.len();
    let num_decode_entries = decoding_entries.len();
    let num_aux_entries = aux_entries.len();
    Ok(quote! {
        pub static TABLE: protocrap::tables::TableWithEntries<
            #num_encode_entries,
            #num_decode_entries,
            #num_aux_entries
        > = protocrap::tables::TableWithEntries {
            encode_entries: [
                #(#encoding_entries),*
            ],
            table: protocrap::tables::Table {
                num_encode_entries: #num_encode_entries as u16,
                num_decode_entries: #num_decode_entries as u16,
                size: core::mem::size_of::<ProtoType>() as u16,
                descriptor: ProtoType::descriptor_proto(),
            },
            decode_entries: [
                #(#decoding_entries),*
            ],
            aux_entries: [
                #(#aux_entries),*
            ],
        };
    })
}

fn field_kind_tokens(field: &&FieldDescriptorProto) -> TokenStream {
    let kind = protocrap::reflection::field_kind_tokens(field);
    let ident = format_ident!("{kind:?}");
    quote! { protocrap::wire::FieldKind::#ident }
}
