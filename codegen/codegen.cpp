#include <cassert>
#include <cstring>

#include <fstream>
#include <iostream>
#include <unordered_map>

#include "google/protobuf/message.h"
#include "google/protobuf/descriptor.h"
#include "google/protobuf/descriptor.pb.h"
#include "google/protobuf/reflection.h"
#include "google/protobuf/io/printer.h"
#include "google/protobuf/io/zero_copy_stream_impl.h"
#include "google/protobuf/wire_format_lite.h"
#include "google/protobuf/wire_format.h"

#include "google/protobuf/compiler/plugin.h"
#include "google/protobuf/compiler/code_generator.h"


namespace gp = google::protobuf;

int Log2FloorNonZero_Portable(uint32_t n) {
    if (n == 0)
        return -1;
    int log = 0;
    uint32_t value = n;
    for (int i = 4; i >= 0; --i) {
        int shift = (1 << i);
        uint32_t x = value >> shift;
        if (x != 0) {
        value = x;
        log += shift;
        }
    }
    assert(value == 1);
    return log;
}

std::string replace_dot_with_underscore(const std::string& input) {
    std::string output = input;
    for (char& c : output) {
        if (c == '.') {
            c = '_';
        }
    }
    return output;
}

std::string rust_field_name(const gp::FieldDescriptor* field) {
    static const std::unordered_set<std::string> KEYWORDS = {
        "type",
    };
    if (KEYWORDS.count(field->name())) {
        return field->name() + "_";
    }
    return field->name();
}

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
            return "protocrap::containers::String";
        case FieldDescriptor::TYPE_BYTES:
            return "protocrap::containers::Bytes";
        case FieldDescriptor::TYPE_BOOL:
            return "bool";
        case FieldDescriptor::TYPE_MESSAGE:
        case FieldDescriptor::TYPE_GROUP:
            return "protocrap::base::Message";
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
            return "protocrap::containers::String";
        case FieldDescriptor::TYPE_BYTES:
            return "protocrap::containers::Bytes";
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
            field_kind = "Varint32"; // TODO fixme
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
            field_kind = "Varint32";
            break;
    }
    if (field->label() == gp::FieldDescriptor::LABEL_REPEATED) {
        field_kind = "Repeated" + field_kind;
    }
    return "protocrap::wire::FieldKind::" + field_kind;
}

void generate_enum_code(const gp::EnumDescriptor* descriptor, gp::io::Printer* printer) {
    auto x = printer->WithVars({{"name", rust_full_name(descriptor)}});
    printer->Emit(R"rs(
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum $name$ {
)rs");
    {
        auto _indent = printer->WithIndent();
        for (int i = 0; i < descriptor->value_count(); ++i) {
            const gp::EnumValueDescriptor* value = descriptor->value(i);
            printer->Emit(
                {{"name", value->name()},
                {"number", std::to_string(value->number())}},
                "\n$name$ = $number$,");
        }
    }
    printer->Emit(R"rs(
}
)rs");
    // Generate the corresponding conversion Rust code for the enum
    printer->Emit(
        {{"name", rust_full_name(descriptor)}},
        R"rs(
impl $name$ {
    pub fn from_i32(value: i32) -> Option<$name$> {
        match value {
)rs");
    {
        auto _indent = printer->WithIndent();
        for (int i = 0; i < descriptor->value_count(); ++i) {
            const gp::EnumValueDescriptor* value = descriptor->value(i);
            printer->Emit(
                {{"name", value->name()},
                    {"type", rust_full_name(descriptor)},
                {"number", std::to_string(value->number())}},
                " $number$ => Some($type$::$name$),\n");
        }
    }
    printer->Emit(
        R"rs(
        _ => None,
        }
    }
    pub fn to_i32(self) -> i32 {
        self as i32
    }
}
    )rs");
}

void generate_code(const gp::Descriptor* descriptor, gp::io::Printer* printer) {
    for (int i = 0; i < descriptor->enum_type_count(); ++i) {
        generate_enum_code(descriptor->enum_type(i), printer);
    }

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
                    {"name", rust_field_name(field)}},
                    "\n$name$: protocrap::containers::RepeatedField<$type$>,");
            } else {
                printer->Emit(
                    {{"type", rust_field_member_type(field)},
                    {"name", rust_field_name(field)}},
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
                    {"name", rust_field_name(field)}},
                    R"rs(
pub fn $name$(&self) -> &[$type$] {
    unsafe { std::mem::transmute(self.$name$.slice()) }
}
pub fn $name$_mut(&mut self) -> &mut protocrap::containers::RepeatedField<protocrap::base::Message> {
    unsafe { std::mem::transmute(&mut self.$name$) }
})rs");
            } else {
                switch (field->type()) {
                    case gp::FieldDescriptor::TYPE_STRING:
                        printer->Emit(
                            {{"type", rust_field_type(field)},
                            {"has_bit_idx", has_bit_idx[field]},
                            {"name", rust_field_name(field)}},
                            R"rs(
pub fn $name$(&self) -> &str {
    &self.$name$
}
pub fn set_$name$(&mut self, value: &str) {
    self.as_object_mut().set_has_bit($has_bit_idx$);
    // self.$name$.assign(value);
})rs");
                        break;
                    case gp::FieldDescriptor::TYPE_BYTES:
                        printer->Emit(
                            {{"type", rust_field_type(field)},
                            {"has_bit_idx", has_bit_idx[field]},
                            {"name", rust_field_name(field)}},
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
                            {"name", rust_field_name(field)}},
                            R"rs(
pub fn $name$(&self) -> Option<&$type$> {
    if self.$name$.0.is_null() {
        None
    } else {
        Some(unsafe { &*(self.$name$.0 as *const $type$) })
    }
}
pub fn $name$_mut(&mut self, arena: &mut crate::arena::Arena) -> &mut $type$ {
    let object = self.$name$;
    if object.0.is_null() {
        let new_object = protocrap::base::Object::create(std::mem::size_of::<$type$>() as u32, arena);
        self.$name$ = protocrap::base::Message(new_object);
    }
    unsafe { &mut *(self.$name$.0 as *mut $type$) }
})rs");
                        break;
                    case gp::FieldDescriptor::TYPE_ENUM:
                        printer->Emit(
                            {{"type", rust_field_type(field)},
                            {"has_bit_idx", has_bit_idx[field]},
                            {"name", rust_field_name(field)}},
                            R"rs(
pub fn $name$(&self) -> Option<$type$> {
    $type$::from_i32(self.$name$)
}
pub fn set_$name$(&mut self, value: $type$) {
    self.as_object_mut().set_has_bit($has_bit_idx$);
    self.$name$ = value.to_i32();
})rs");
                        break;
                    default:
                        printer->Emit(
                            {{"type", rust_field_type(field)},
                            {"has_bit_idx", has_bit_idx[field]},
                            {"name", rust_field_name(field)}},
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
    if (max_field_number > 2047) {
        std::cerr << "Field numbers > 2047 not supported yet\n";
        std::exit(1);
    }
    int num_masked_bits = 4;  // Always cover the field numbers 0..15
    if (max_field_number > 15) {
        // ilog2(max_field_number) gives the highest set bit position (0-based).
        // we want total bits + continuation bit, so +2.
        num_masked_bits = Log2FloorNonZero_Portable(max_field_number) + 2;
    }
    int num_masked = 1 << num_masked_bits;
    int mask = (num_masked - 1) << 3;
    printer->Emit(
        {
            {"num_masked", num_masked},
            {"mask", mask},
         {"num_entries", max_field_number + 1},
         {"num_aux_entries", num_aux_entries}},
        R"rs(

impl protocrap::Protobuf for $name$ {
    fn encoding_table() -> &'static [crate::encoding::TableEntry] {
        &ENCODING_TABLE_$name$.0
    }
    fn decoding_table() -> &'static crate::decoding::Table {
        &DECODING_TABLE_$name$.0
    }
}

static DECODING_TABLE_$name$: protocrap::decoding::TableWithEntries<$num_masked$, $num_entries$, $num_aux_entries$> = protocrap::decoding::TableWithEntries(
    crate::decoding::Table {
        mask: $mask$,
        size: std::mem::size_of::<$name$>() as u16,
    },
    [)rs");
    for (int i = 0; i < num_masked; ++i) {
        int field_number = (i & 15) | (((i >> 5) << 4) * ((i >> 4) & 1));
        const gp::FieldDescriptor* field = descriptor->FindFieldByNumber(field_number);
        std::string field_kind_str;
        if (field) {
            field_kind_str = field_kind(field);
        } else {
            field_kind_str = "protocrap::wire::FieldKind::Unknown";
        }
        printer->Emit(
            {{"kind", field_kind_str}},
            "$kind$ ,\n");
    }
    printer->Emit(R"rs(  ],
    [)rs");

    int aux_idx = 0;
    for (int field_number = 0; field_number <= max_field_number; ++field_number) {
        const gp::FieldDescriptor* field = descriptor->FindFieldByNumber(field_number);
        if (field) {
            auto field_kind_str = field_kind(field);
            if (field->message_type()) {
                // No has bit, nullptr indicates absence
                printer->Emit(
                    {{"has_bit", has_bit_idx[field]},
                    {"aux_idx", aux_idx++},
                    {"num_masked", num_masked},
                    {"num_entries", max_field_number + 1},
                    {"num_aux_entries", num_aux_entries}},
                    "protocrap::decoding::TableEntry((std::mem::offset_of!(protocrap::decoding::TableWithEntries<$num_masked$, $num_entries$, $num_aux_entries$>, 3) + $aux_idx$ * std::mem::size_of::<protocrap::decoding::AuxTableEntry>()) as u16),\n");
            } else {
                auto has_bit = has_bit_idx[field] << 10;
                printer->Emit(
                    {{"has_bit", has_bit},
                    {"field_name", rust_field_name(field)}},
                    "protocrap::decoding::TableEntry(std::mem::offset_of!($name$, $field_name$) as u16 + $has_bit$), \n");
            }
        } else {
            printer->Emit("protocrap::decoding::TableEntry(0), \n");
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
                 {"field_name", rust_field_name(field)},
                    {"child_type_name", rust_full_name(field->message_type())}},
                "protocrap::decoding::AuxTableEntry {offset: std::mem::offset_of!($name$, $field_name$) as u32, child_table: &DECODING_TABLE_$child_type_name$.0},\n");
        }
    }
    printer->Emit({
        {"num_entries", descriptor->field_count()},
        {"num_aux_entries", num_aux_entries}},
        R"rs(]
);

static ENCODING_TABLE_$name$: protocrap::encoding::TableWithEntries<$num_entries$, $num_aux_entries$> = protocrap::encoding::TableWithEntries(
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
                {"field_name", rust_field_name(field)}},
                "protocrap::encoding::TableEntry {has_bit: $has_bit$, kind: $kind$, offset: (std::mem::offset_of!(protocrap::encoding::TableWithEntries<$num_entries$, $num_aux_entries$>, 1) + $aux_idx$ * std::mem::size_of::<protocrap::encoding::AuxTableEntry>()) as u16, encoded_tag: $encoded_tag$},\n");
        } else {
            printer->Emit(
                {{"has_bit", has_bit_idx[field]},
                {"kind", field_kind_str},
                {"encoded_tag", tag},
                {"field_name", rust_field_name(field)}},
                "protocrap::encoding::TableEntry {has_bit: $has_bit$, kind: $kind$, offset: std::mem::offset_of!($name$, $field_name$) as u16, encoded_tag: $encoded_tag$},\n");
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
                 {"field_name", rust_field_name(field)},
                    {"child_type_name", rust_full_name(field->message_type())}},
                "protocrap::encoding::AuxTableEntry {offset: std::mem::offset_of!($name$, $field_name$), child_table: &ENCODING_TABLE_$child_type_name$.0},\n");
        }
    }
    printer->Emit(R"rs(]);
        
)rs");


    for (int i = 0; i < descriptor->nested_type_count(); ++i) {
        generate_code(descriptor->nested_type(i), printer);
    }
}

std::string descriptor_name(const gp::Message& msg) {
    return "test";
}

std::string value(const gp::Message& msg, const gp::FieldDescriptor* field, int index) {
    auto refl = msg.GetReflection();
    auto has_field = field->is_repeated() || refl->HasField(msg, field);
    switch (field->type()) {
        case gp::FieldDescriptor::TYPE_INT32:
        case gp::FieldDescriptor::TYPE_SINT32:
        case gp::FieldDescriptor::TYPE_SFIXED32:
            if (field->is_repeated()) {
                return std::to_string(refl->GetRepeatedInt32(msg, field, index));
            }
            if (!has_field) {
                return "0";
            }
            return std::to_string(refl->GetInt32(msg, field));
        case gp::FieldDescriptor::TYPE_INT64:
        case gp::FieldDescriptor::TYPE_SINT64:
        case gp::FieldDescriptor::TYPE_SFIXED64:
            if (field->is_repeated()) {
                return std::to_string(refl->GetRepeatedInt64(msg, field, index));
            }
            if (!has_field) {
                return "0";
            }
            return std::to_string(refl->GetInt64(msg, field));
        case gp::FieldDescriptor::TYPE_UINT32:
        case gp::FieldDescriptor::TYPE_FIXED32:
            if (field->is_repeated()) {
                return std::to_string(refl->GetRepeatedUInt32(msg, field, index));
            }
            if (!has_field) {
                return "0";
            }
            return std::to_string(refl->GetUInt32(msg, field));
        case gp::FieldDescriptor::TYPE_UINT64:
        case gp::FieldDescriptor::TYPE_FIXED64:
            if (field->is_repeated()) {
                return std::to_string(refl->GetRepeatedUInt64(msg, field, index));
            }
            if (!has_field) {
                return "0";
            }
            return std::to_string(refl->GetUInt64(msg, field));
        case gp::FieldDescriptor::TYPE_BOOL:
            if (field->is_repeated()) {
                return refl->GetRepeatedBool(msg, field, index) ? "true" : "false";
            }
            if (!has_field) {
                return "false";
            }
            return refl->GetBool(msg, field) ? "true" : "false";
        case gp::FieldDescriptor::TYPE_FLOAT:
            if (field->is_repeated()) {
                return std::to_string(refl->GetRepeatedFloat(msg, field, index));
            }
            if (!has_field) {
                return "0.0";
            }
            return std::to_string(refl->GetFloat(msg, field));
        case gp::FieldDescriptor::TYPE_DOUBLE:
            if (field->is_repeated()) {
                return std::to_string(refl->GetRepeatedDouble(msg, field, index));
            }
            if (!has_field) {
                return "0.0";
            }
            return std::to_string(refl->GetDouble(msg, field));
        case gp::FieldDescriptor::TYPE_STRING:
            if (field->is_repeated()) {
                return "protocrap::containers::String::from_static_slice(\"" + refl->GetRepeatedString(msg, field, index) + "\")";
            }
            if (!has_field) {
                return "protocrap::containers::String::new()";
            }
            return "protocrap::containers::String::from_static_slice(\"" + refl->GetString(msg, field) + "\")";
        case gp::FieldDescriptor::TYPE_BYTES:
            if (field->is_repeated()) {
                return "protocrap::containers::Bytes::from_static_slice(\"" + refl->GetRepeatedString(msg, field, index) + "\")";
            }
            if (!has_field) {
                return "protocrap::containers::Bytes::new()";
            }
            return "protocrap::containers::Bytes::from_static_slice(\"" + refl->GetString(msg, field) + "\")";
        case gp::FieldDescriptor::TYPE_ENUM: {
            if (field->is_repeated()) {
                auto enum_value = refl->GetRepeatedEnum(msg, field, index);
                return std::to_string(enum_value->number());
            }
            return std::to_string(refl->GetEnum(msg, field)->number());
        case gp::FieldDescriptor::TYPE_MESSAGE:
        case gp::FieldDescriptor::TYPE_GROUP:
            std::cerr << "Error: Unsupported field type" << std::endl;
            std::terminate();
        }
    }
}   

void generate_descriptor_data(
    const gp::Message& msg,
    gp::io::Printer* printer
) {
    auto descriptor = msg.GetDescriptor();
    auto refl = msg.GetReflection();
    printer->Emit(
        {
            {"type", rust_full_name(descriptor)},
        },
        "$type$ { has_bits: [\n");
    uint32_t has_bits = 0;
    int bit_idx = 0;
    for (size_t i = 0; i < descriptor->field_count(); ++i) {
        auto field = descriptor->field(i);
        if (field->is_repeated() || field->message_type()) {
            continue;
        }
        if (refl->HasField(msg, field)) {
            has_bits |= (1 << bit_idx);
        }
        ++bit_idx;
        if (bit_idx == 32) {
            printer->Emit(std::to_string(has_bits) + "u32, ");
            has_bits = 0;
            bit_idx = 0;
        }
    }
    if (bit_idx != 0) {
        printer->Emit(std::to_string(has_bits) + "u32, ");
    }
    printer->Emit("],\n");
    int number_of_fields = descriptor->field_count();
    for (int i = 0; i < number_of_fields; ++i) {
        //printf("Processing field %d / %d %s\n", i + 1, number_of_fields, descriptor->field(i)->name().c_str());
        const gp::FieldDescriptor* field = descriptor->field(i);
        if (field == nullptr) {
            continue;
        }
        if (field->is_repeated()) {
            // Repeated fields not supported yet
            printer->Emit(
                {
                    {"field_name", rust_field_name(field)},
                },
                "  $field_name$: protocrap::containers::RepeatedField::from_static_slice(&[\n");
            if (field->message_type()) {
                int num_repeated = refl->FieldSize(msg, field);
                for (int j = 0; j < num_repeated; ++j) {
                    printer->Emit("protocrap::base::Message(&");
                    generate_descriptor_data(
                        refl->GetRepeatedMessage(msg, field, j),
                        printer);
                    printer->Emit(" as *const _ as *mut protocrap::base::Object),\n");
                }
            } else {
                int num_repeated = refl->FieldSize(msg, field);
                for (int j = 0; j < num_repeated; ++j) {
                    auto val = value(msg, field, j);
                    printer->Emit(
                        {
                            {"value", val}
                        },
                        "$value$, ");
                }
            }
            printer->Emit("]),\n");
            continue;
        }
        if (field->message_type()) {
            printer->Emit(
                {
                    {"field_name", rust_field_name(field)},
                },
                " $field_name$: ");
            if (refl->HasField(msg, field)) {
                printer->Emit(" protocrap::base::Message(&");
                generate_descriptor_data(
                    refl->GetMessage(msg, field),
                    printer);
                printer->Emit(" as *const _ as *mut protocrap::base::Object)");
            } else {
                printer->Emit("protocrap::base::Message(std::ptr::null_mut())");
            }
            printer->Emit(",\n");
        } else {
            auto val = value(msg, field, 0);
            printer->Emit(
                {
                    {"field_name", rust_field_name(field)},
                    {"value", val}
                },
                " $field_name$: $value$,\n");
        }
    }
    printer->Emit("}\n");
}

class ProtocrapGenerator : public google::protobuf::compiler::CodeGenerator {
public:
    bool Generate(
        const google::protobuf::FileDescriptor* file,
        const std::string& parameter,
        google::protobuf::compiler::GeneratorContext* context,
        std::string* error
    ) const override {
        // Your existing generate_code logic
        std::string output_filename = file->name();
        // Replace .proto with .rs
        output_filename = output_filename.substr(0, output_filename.size() - 6) + ".pc.rs";
        
        auto* output = context->Open(output_filename);
        google::protobuf::io::Printer printer(output, '$');
        
        printer.Emit(R"rs(
// Automatically generated Rust code from protobuf definitions.\n\n");

use protocrap::Protobuf;

)rs");

        for (int i = 0; i < file->enum_type_count(); ++i) {
            generate_enum_code(file->enum_type(i), &printer);
        }

        for (int i = 0; i < file->message_type_count(); ++i) {
            generate_code(file->message_type(i), &printer);
        }

        gp::FileDescriptorProto file_proto;
        file->CopyTo(&file_proto);
        printer.Emit("static FILE_DESCRIPTOR_PROTO: google_protobuf_FileDescriptorProto = ");
        generate_descriptor_data(file_proto, &printer);
        printer.Emit(";\n");

        return true;
    }
};

int main(int argc, char* argv[]) {
    ProtocrapGenerator generator;
    return google::protobuf::compiler::PluginMain(argc, argv, &generator);
}
