use rust_protobuf::base;

fn rust_group_name(proto_path: &[String], name: &str) -> String {
    if proto_path.is_empty() {
        name.to_string()
    } else {
        format!("{}_{}", proto_path.join("."), name)
    }
}

fn rust_qualified_name(name: &str) -> String {
    name.replace('.', "::")
}

fn rust_primitive_type(
    field: &protobuf_parser::Field,
    proto_path: &[String],
    field_name: &str,
) -> String {
    match &field.typ {
        protobuf_parser::FieldType::Double => "f64".to_string(),
        protobuf_parser::FieldType::Float => "f32".to_string(),
        protobuf_parser::FieldType::Int32 => "i32".to_string(),
        protobuf_parser::FieldType::Int64 => "i64".to_string(),
        protobuf_parser::FieldType::Uint32 => "u32".to_string(),
        protobuf_parser::FieldType::Uint64 => "u64".to_string(),
        protobuf_parser::FieldType::Sint32 => "i32".to_string(),
        protobuf_parser::FieldType::Sint64 => "i64".to_string(),
        protobuf_parser::FieldType::Fixed32 => "u32".to_string(),
        protobuf_parser::FieldType::Fixed64 => "u64".to_string(),
        protobuf_parser::FieldType::Sfixed32 => "i32".to_string(),
        protobuf_parser::FieldType::Sfixed64 => "i64".to_string(),
        protobuf_parser::FieldType::Bool => "bool".to_string(),
        protobuf_parser::FieldType::String | protobuf_parser::FieldType::Bytes => {
            "protobuf::repeated_field::Bytes".to_string()
        }
        protobuf_parser::FieldType::Group(_) => rust_group_name(proto_path, field_name),
        protobuf_parser::FieldType::Map(_) => panic!("Map types not supported"),
        protobuf_parser::FieldType::MessageOrEnum(name) => rust_qualified_name(&name),
    }
}

fn rust_field_type(field: &protobuf_parser::Field, proto_path: &[String]) -> String {
    let base_type = rust_primitive_type(field, proto_path, &field.name);
    match field.rule {
        protobuf_parser::Rule::Optional | protobuf_parser::Rule::Required => base_type,
        protobuf_parser::Rule::Repeated => {
            format!("protobuf::repeated_field::RepeatedField<{}>", base_type)
        }
    }
}

fn generate_message_code(message: &protobuf_parser::Message, proto_path: &mut Vec<String>) {
    proto_path.push(message.name.clone());
    println!("pub struct {} {{", message.name);
    for field in &message.fields {
        println!(
            "    {}: {},",
            field.name,
            rust_field_type(field, &proto_path)
        );
    }
    println!("}}\n");

    println!("impl {} {{", message.name);
    for field in &message.fields {
        match field.rule {
            protobuf_parser::Rule::Optional | protobuf_parser::Rule::Required => {
                let rust_type = rust_field_type(field, &proto_path);
                println!("    pub fn {}(&self) -> {} {{", field.name, rust_type);
                println!("        self.{}", field.name);
                println!("    }}\n");
                println!(
                    "    pub fn set_{}(&mut self, value: {}) {{",
                    field.name, rust_type
                );
                println!("        self.{} = value;", field.name);
                println!("    }}\n");
            }
            protobuf_parser::Rule::Repeated => {
                let base_type = rust_primitive_type(field, &proto_path, &field.name);
                let rust_type = rust_field_type(field, &proto_path);
                println!("    pub fn {}(&self) -> &[{}] {{", field.name, base_type);
                println!("        &self.{}", field.name);
                println!("    }}\n");
                println!(
                    "    pub fn {}_mut(&mut self) -> &mut [{}] {{",
                    field.name, base_type
                );
                println!("        &mut self.{}", field.name);
                println!("    }}\n");
                println!(
                    "    pub fn add_{}(&mut self, value: {}) {{",
                    field.name, base_type
                );
                println!("        self.{}.push(value);", field.name);
                println!("    }}\n");
                println!(
                    "    pub fn pop_{}(&mut self) -> Option<{}> {{",
                    field.name, base_type
                );
                println!("        self.{}.pop()", field.name);
                println!("    }}\n");
                println!(
                    "    pub fn remove_{}(&mut self, index: usize) {{",
                    field.name
                );
                println!("        self.{}.remove(index);", field.name);
                println!("    }}\n");
            }
        }
        println!("}}");
    }

    for nested_message in &message.messages {
        generate_message_code(nested_message, proto_path);
    }
    proto_path.pop();
}

fn main() {
    let file = std::fs::read_to_string("proto/example.proto").unwrap();
    let descriptor = protobuf_parser::FileDescriptor::parse(&file).unwrap();

    println!("// Generated Rust code from proto/example.proto\n");

    for message in &descriptor.messages {
        generate_message_code(&message, &mut Vec::new());
    }

    println!("{:?}", descriptor);
}
