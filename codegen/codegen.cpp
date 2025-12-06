#include <cassert>
#include <cstring>

#include <fstream>
#include <iostream>
#include <unordered_map>

#include "proto/test.pb.h"

#include "google/protobuf/message.h"
#include "google/protobuf/io/printer.h"
#include "google/protobuf/io/zero_copy_stream_impl.h"
#include "google/protobuf/wire_format_lite.h"
#include "google/protobuf/wire_format.h"

namespace gp = google::protobuf;

std::string rust_full_name(const gp::Descriptor* descriptor) {
    std::string full_name = descriptor->full_name();
    for (char& c : full_name) {
        if (c == '.') {
            c = '_';
        }
    }
    return full_name;
}

std::string rust_full_name(const gp::EnumDescriptor* descriptor) {
    std::string full_name = descriptor->full_name();
    for (char& c : full_name) {
        if (c == '.') {
            c = '_';
        }
    }
    return full_name;
}


std::string rust_field_member_type(const gp::FieldDescriptor* field) {
    using gp::FieldDescriptor;
    switch (field->type()) {
        case FieldDescriptor::TYPE_INT32:
        case FieldDescriptor::TYPE_SINT32:
        case FieldDescriptor::TYPE_SFIXED32:
            return "i32";
        case FieldDescriptor::TYPE_INT64:
        case FieldDescriptor::TYPE_SINT64:
        case FieldDescriptor::TYPE_SFIXED64:
            return "i64";
        case FieldDescriptor::TYPE_UINT32:
        case FieldDescriptor::TYPE_FIXED32:
            return "u32";
        case FieldDescriptor::TYPE_UINT64:
        case FieldDescriptor::TYPE_FIXED64:
            return "u64";
        case FieldDescriptor::TYPE_FLOAT:
            return "f32";
        case FieldDescriptor::TYPE_DOUBLE:
            return "f64";
        case FieldDescriptor::TYPE_STRING:
        case FieldDescriptor::TYPE_BYTES:
            return "protobuf::containers::Bytes";
        case FieldDescriptor::TYPE_BOOL:
            return "bool";
        case FieldDescriptor::TYPE_MESSAGE:
        case FieldDescriptor::TYPE_GROUP:
            return "*mut protobuf::base::Object";
        case FieldDescriptor::TYPE_ENUM:
            return "i32";
    }
    __builtin_unreachable();
}

std::string rust_field_type(const gp::FieldDescriptor* field) {
    using gp::FieldDescriptor;
    switch (field->type()) {
        case FieldDescriptor::TYPE_INT32:
        case FieldDescriptor::TYPE_SINT32:
        case FieldDescriptor::TYPE_SFIXED32:
            return "i32";
        case FieldDescriptor::TYPE_INT64:
        case FieldDescriptor::TYPE_SINT64:
        case FieldDescriptor::TYPE_SFIXED64:
            return "i64";
        case FieldDescriptor::TYPE_UINT32:
        case FieldDescriptor::TYPE_FIXED32:
            return "u32";
        case FieldDescriptor::TYPE_UINT64:
        case FieldDescriptor::TYPE_FIXED64:
            return "u64";
        case FieldDescriptor::TYPE_FLOAT:
            return "f32";
        case FieldDescriptor::TYPE_DOUBLE:
            return "f64";
        case FieldDescriptor::TYPE_STRING:
        case FieldDescriptor::TYPE_BYTES:
            return "protobuf::containers::Bytes";
        case FieldDescriptor::TYPE_BOOL:
            return "bool";
        case FieldDescriptor::TYPE_MESSAGE:
        case FieldDescriptor::TYPE_GROUP:
            return rust_full_name(field->message_type());
        case FieldDescriptor::TYPE_ENUM:
            return rust_full_name(field->enum_type());
    }
    __builtin_unreachable();
}

std::string field_kind(const gp::FieldDescriptor* field) {
    std::string field_kind;
    switch (field->type()) {
        case gp::FieldDescriptor::TYPE_INT32:
        case gp::FieldDescriptor::TYPE_UINT32:
            field_kind = "Varint32";
            break;
        case gp::FieldDescriptor::TYPE_SINT32:
            field_kind = "Varint32Zigzag";
            break;
        case gp::FieldDescriptor::TYPE_SFIXED32:
        case gp::FieldDescriptor::TYPE_FLOAT:
        case gp::FieldDescriptor::TYPE_FIXED32:
            field_kind = "Fixed32";
            break;
        case gp::FieldDescriptor::TYPE_INT64:
        case gp::FieldDescriptor::TYPE_UINT64:
            field_kind = "Varint64";
            break;
        case gp::FieldDescriptor::TYPE_SINT64:
            field_kind = "Varint64Zigzag";
            break;
        case gp::FieldDescriptor::TYPE_SFIXED64:
        case gp::FieldDescriptor::TYPE_DOUBLE:
        case gp::FieldDescriptor::TYPE_FIXED64:
            field_kind = "Fixed64";
            break;
        case gp::FieldDescriptor::TYPE_BOOL:
            assert(false && "bool field kind not implemented yet");
            break;
        case gp::FieldDescriptor::TYPE_STRING:
        case gp::FieldDescriptor::TYPE_BYTES:
            field_kind = "Bytes";
            break;
        case gp::FieldDescriptor::TYPE_MESSAGE:
            field_kind = "Message";
            break;
        case gp::FieldDescriptor::TYPE_GROUP:
            field_kind = "Group";
            break;
        case gp::FieldDescriptor::TYPE_ENUM:
            assert(false && "enum field kind not implemented yet");
            break;
    }
    if (field->label() == gp::FieldDescriptor::LABEL_REPEATED) {
        field_kind = "Repeated" + field_kind;
    }
    return "protobuf::wire::FieldKind::" + field_kind;
}

void generate_code(const gp::Descriptor* descriptor, gp::io::Printer* printer) {
    auto x = printer->WithVars({{"name", rust_full_name(descriptor)}});
    int number_of_has_bits = 0;
    std::unordered_map<const gp::FieldDescriptor*, int> has_bit_idx;
    for (int i = 0; i < descriptor->field_count(); ++i) {
        const gp::FieldDescriptor* field = descriptor->field(i);
        if (field->message_type() || field->label() == gp::FieldDescriptor::LABEL_REPEATED) {
            // Messages and groups do not have has bits, as their presence is
            // indicated by a null pointer.
            continue;
        }
        has_bit_idx[field] = number_of_has_bits++;
    }
    printer->Emit(
        {{"N", (number_of_has_bits + 31) / 32}},
        R"rs(
#[repr(C)]
#[derive(Debug, Default)]
pub struct $name$ {
  has_bits: [u32; $N$],
)rs");
    {
        auto _indent = printer->WithIndent();
        for (int i = 0; i < descriptor->field_count(); ++i) {
            const gp::FieldDescriptor* field = descriptor->field(i);
            if (field->label() == gp::FieldDescriptor::LABEL_REPEATED) {
                printer->Emit(
                    {{"type", rust_field_member_type(field)},
                    {"name", field->name()}},
                    "\n$name$: protobuf::containers::RepeatedField<$type$>,");
            } else {
                printer->Emit(
                    {{"type", rust_field_member_type(field)},
                    {"name", field->name()}},
                    "\n$name$: $type$,");
            }
        }
    }
    printer->Emit(R"rs(
}

impl $name$ {)rs");
    {
        auto _indent = printer->WithIndent();
        for (int i = 0; i < descriptor->field_count(); ++i) {
            const gp::FieldDescriptor* field = descriptor->field(i);
            if (field->label() == gp::FieldDescriptor::LABEL_REPEATED) {
                printer->Emit(
                    {{"type", rust_field_type(field)},
                    {"name", field->name()}},
                    R"rs(
pub fn $name$(&self) -> &[$type$] {
    unsafe { std::mem::transmute(self.$name$.slice()) }
}
pub fn $name$_mut(&mut self) -> &mut protobuf::containers::RepeatedField<*mut $type$> {
    unsafe { std::mem::transmute(&mut self.$name$) }
})rs");
            } else {
                switch (field->type()) {
                    case gp::FieldDescriptor::TYPE_STRING:
                    case gp::FieldDescriptor::TYPE_BYTES:
                        printer->Emit(
                            {{"type", rust_field_type(field)},
                            {"has_bit_idx", has_bit_idx[field]},
                            {"name", field->name()}},
                            R"rs(
pub fn $name$(&self) -> &[u8] {
    &self.$name$
}
pub fn set_$name$(&mut self, value: &[u8]) {
    self.as_object_mut().set_has_bit($has_bit_idx$);
    self.$name$.assign(value);
})rs");
                        break;
                    case gp::FieldDescriptor::TYPE_MESSAGE:
                    case gp::FieldDescriptor::TYPE_GROUP:
                        printer->Emit(
                            {{"type", rust_field_type(field)},
                            {"name", field->name()}},
                            R"rs(
pub fn $name$(&self) -> Option<&$type$> {
    if self.$name$.is_null() {
        None
    } else {
        Some(unsafe { &*(self.$name$ as *const $type$) })
    }
}
pub fn $name$_mut(&mut self, arena: &mut crate::arena::Arena) -> &mut $type$ {
    let object = self.$name$;
    if object.is_null() {
        let new_object = protobuf::base::Object::create(std::mem::size_of::<$type$>() as u32, arena);
        self.$name$ = new_object;
    }
    unsafe { &mut *(self.$name$ as *mut $type$) }
})rs");
                        break;
                    case gp::FieldDescriptor::TYPE_ENUM:
                        printer->Emit(
                            {{"type", rust_field_type(field)},
                            {"has_bit_idx", has_bit_idx[field]},
                            {"name", field->name()}},
                            R"rs(
pub fn $name$(&self) -> Option<$type$> {
    self.$name$
}
pub fn set_$name$(&mut self, value: $type$) {
    self.as_object_mut().set_has_bit($has_bit_idx$);
    self.$name$ = value;
})rs");
                        break;
                    default:
                        printer->Emit(
                            {{"type", rust_field_type(field)},
                            {"has_bit_idx", has_bit_idx[field]},
                            {"name", field->name()}},
                            R"rs(
pub fn $name$(&self) -> $type$ {
    self.$name$
}
pub fn set_$name$(&mut self, value: $type$) {
    self.as_object_mut().set_has_bit($has_bit_idx$);
    self.$name$ = value;
})rs");
                        break;
                }
            }
        }

        for (int i = 0; i < descriptor->nested_type_count(); ++i) {
            printer->Emit(
                {{"name", descriptor->nested_type(i)->name()},
                 {"rust_name", rust_full_name(descriptor->nested_type(i))}}
                , "\n// type $name$ = $rust_name$;");
        }
    }
    printer->Emit("\n}\n");

    int num_aux_entries = 0;
    int max_field_number = 0;
    for (int i = 0; i < descriptor->field_count(); ++i) {
        const gp::FieldDescriptor* field = descriptor->field(i);
        if (field->type() == gp::FieldDescriptor::TYPE_MESSAGE ||
            field->type() == gp::FieldDescriptor::TYPE_GROUP) {
            ++num_aux_entries;
        }
        max_field_number = std::max(max_field_number, field->number());
    }
    printer->Emit(
        {{"num_entries", max_field_number + 1},
         {"num_aux_entries", num_aux_entries}},
        R"rs(

impl protobuf::Protobuf for $name$ {
    fn encoding_table() -> &'static [crate::encoding::TableEntry] {
        &ENCODING_TABLE_$name$.0
    }
    fn decoding_table() -> &'static crate::decoding::Table {
        &DECODING_TABLE_$name$.0
    }
}

static DECODING_TABLE_$name$: protobuf::decoding::TableWithEntries<$num_entries$, $num_aux_entries$> = protobuf::decoding::TableWithEntries(
    crate::decoding::Table {
        num_entries: $num_entries$,
        size: std::mem::size_of::<$name$>() as u32,
    },
    [)rs");

    int aux_idx = 0;
    for (int field_number = 0; field_number <= max_field_number; ++field_number) {
        const gp::FieldDescriptor* field = descriptor->FindFieldByNumber(field_number);
        if (field) {
            auto field_kind_str = field_kind(field);
            if (field->message_type()) {
                printer->Emit(
                    {{"has_bit", has_bit_idx[field]},
                    {"aux_idx", aux_idx++},
                    {"kind", field_kind_str},
                    {"num_entries", max_field_number + 1},
                    {"num_aux_entries", num_aux_entries},
                    {"field_name", field->name()}},
                    "protobuf::decoding::TableEntry {has_bit: $has_bit$, kind: $kind$, offset: (std::mem::offset_of!(protobuf::decoding::TableWithEntries<$num_entries$, $num_aux_entries$>, 2) + $aux_idx$ * std::mem::size_of::<protobuf::decoding::AuxTableEntry>()) as u16},\n");
            } else {
                printer->Emit(
                    {{"has_bit", has_bit_idx[field]},
                    {"kind", field_kind_str},
                    {"field_name", field->name()}},
                    "protobuf::decoding::TableEntry {has_bit: $has_bit$, kind: $kind$, offset: std::mem::offset_of!($name$, $field_name$) as u16},\n");
            }
        } else {
            printer->Emit("protobuf::decoding::TableEntry {has_bit: 0, kind: protobuf::wire::FieldKind::Unknown, offset: 0}, \n");
        }
    }
    printer->Emit(R"rs(  ],
        [
)rs");

    for (int i = 0; i < descriptor->field_count(); ++i) {
        const gp::FieldDescriptor* field = descriptor->field(i);
        if (field->message_type()) {
            printer->Emit(
                {{"offset", std::to_string(i)},
                 {"field_name", field->name()},
                    {"child_type_name", rust_full_name(field->message_type())}},
                "protobuf::decoding::AuxTableEntry {offset: std::mem::offset_of!($name$, $field_name$) as u32, child_table: &DECODING_TABLE_$child_type_name$.0},\n");
        }
    }
    printer->Emit({
        {"num_entries", descriptor->field_count()},
        {"num_aux_entries", num_aux_entries}},
        R"rs(]
);

static ENCODING_TABLE_$name$: protobuf::encoding::TableWithEntries<$num_entries$, $num_aux_entries$> = protobuf::encoding::TableWithEntries(
[
)rs");
    aux_idx = 0;
    for (int i = 0; i < descriptor->field_count(); ++i) {
        const gp::FieldDescriptor* field = descriptor->field(i);
        auto field_kind_str = field_kind(field);
        auto tag = gp::internal::WireFormatLite::MakeTag(field->number(),
            gp::internal::WireFormat::WireTypeForFieldType(field->type()));
        // TODO: optimize tag
        if (field->message_type()) {
            printer->Emit(
                {{"has_bit", has_bit_idx[field]},
                {"aux_idx", aux_idx++},
                {"kind", field_kind_str},
                {"num_entries", descriptor->field_count()},
                {"num_aux_entries", num_aux_entries},
                {"encoded_tag", tag},
                {"field_name", field->name()}},
                "protobuf::encoding::TableEntry {has_bit: $has_bit$, kind: $kind$, offset: (std::mem::offset_of!(protobuf::encoding::TableWithEntries<$num_entries$, $num_aux_entries$>, 1) + $aux_idx$ * std::mem::size_of::<protobuf::encoding::AuxTableEntry>()) as u16, encoded_tag: $encoded_tag$},\n");
        } else {
            printer->Emit(
                {{"has_bit", has_bit_idx[field]},
                {"kind", field_kind_str},
                {"encoded_tag", tag},
                {"field_name", field->name()}},
                "protobuf::encoding::TableEntry {has_bit: $has_bit$, kind: $kind$, offset: std::mem::offset_of!($name$, $field_name$) as u16, encoded_tag: $encoded_tag$},\n");
        }
    }
    printer->Emit(R"rs(
], [
)rs");
    // TODO reuse aux entries from decoding table
    for (int i = 0; i < descriptor->field_count(); i++) {
        const gp::FieldDescriptor* field = descriptor->field(i);
        if (field->message_type()) {
            printer->Emit(
                {{"offset", std::to_string(i)},
                 {"field_name", field->name()},
                    {"child_type_name", rust_full_name(field->message_type())}},
                "protobuf::encoding::AuxTableEntry {offset: std::mem::offset_of!($name$, $field_name$), child_table: &ENCODING_TABLE_$child_type_name$.0},\n");
        }
    }
    printer->Emit(R"rs(]);)rs");


    for (int i = 0; i < descriptor->nested_type_count(); ++i) {
        generate_code(descriptor->nested_type(i), printer);
    }
}



int main() {
    Test msg;
    gp::io::FileOutputStream file_output(1);
    gp::io::Printer printer(&file_output, '$');

    printer.Emit(R"rs(
// Automatically generated Rust code from protobuf definitions.\n\n");

#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)]
#![allow(clippy::erasing_op)]
#![allow(clippy::identity_op)]

use crate as protobuf;
use protobuf::Protobuf;

)rs");

    generate_code(msg.descriptor(), &printer);

    return 0;
}