#include <fstream>
#include <iostream>

#include "example.pb.h"

#include "google/protobuf/message.h"
#include "google/protobuf/io/printer.h"
#include "google/protobuf/io/zero_copy_stream_impl.h"

namespace gp = google::protobuf;

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
            return "protobuf::Bytes";
        case FieldDescriptor::TYPE_BOOL:
            return "bool";
        case FieldDescriptor::TYPE_MESSAGE:
        case FieldDescriptor::TYPE_GROUP:
            return field->message_type()->full_name();
        default:
            return "unimplemented_type";
    }
}

void generate_example(const gp::Descriptor* descriptor, gp::io::Printer* printer) {
    auto x = printer->WithVars({{"name", descriptor->name()}});
    printer->Emit(R"rs(
// Generating code for message: $name$
struct $name$ {)rs");
    {
        auto _indent = printer->WithIndent();
        for (int i = 0; i < descriptor->field_count(); ++i) {
            const gp::FieldDescriptor* field = descriptor->field(i);
            printer->Emit(
                {{"type", rust_field_type(field)},
                 {"name", field->name()}},
                "\n$name$: $type$");
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
            } else {
                printer->Emit(
                    {{"type", rust_field_type(field)},
                    {"name", field->name()}},
                    R"rs(
pub fn $name$(&self) -> &$type$ {
    self.$name$
}
pub fn set_$name$(&mut self, value: $type$) {
    self.$name$ = value;
})rs");
            }
        }   
    }
    printer->Emit("\n}\n");

    for (int i = 0; i < descriptor->nested_type_count(); ++i) {
        generate_example(descriptor->nested_type(i), printer);
    }
}

int main() {
    Test msg;
    gp::io::FileOutputStream file_output(1);
    gp::io::Printer printer(&file_output, '$');

    generate_example(msg.descriptor(), &printer);

    return 0;
}