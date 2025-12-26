use crate::{
    Protobuf, ProtobufExt,
    arena::Arena,
    base::{Message, Object},
    containers::{Bytes, String},
    google::protobuf::{
        DescriptorProto::ProtoType as DescriptorProto,
        FieldDescriptorProto::{Label, ProtoType as FieldDescriptorProto, Type},
        FileDescriptorProto::ProtoType as FileDescriptorProto,
    },
    tables::Table,
    wire,
};

pub fn field_kind_tokens(field: &&FieldDescriptorProto) -> wire::FieldKind {
    if field.label().unwrap() == Label::LABEL_REPEATED {
        match field.r#type().unwrap() {
            Type::TYPE_INT32 => wire::FieldKind::RepeatedInt32,
            Type::TYPE_UINT32 => wire::FieldKind::RepeatedVarint32,
            Type::TYPE_INT64 | Type::TYPE_UINT64 => wire::FieldKind::RepeatedVarint64,
            Type::TYPE_SINT32 => wire::FieldKind::RepeatedVarint32Zigzag,
            Type::TYPE_SINT64 => wire::FieldKind::RepeatedVarint64Zigzag,
            Type::TYPE_FIXED32 | Type::TYPE_SFIXED32 | Type::TYPE_FLOAT => {
                wire::FieldKind::RepeatedFixed32
            }
            Type::TYPE_FIXED64 | Type::TYPE_SFIXED64 | Type::TYPE_DOUBLE => {
                wire::FieldKind::RepeatedFixed64
            }
            Type::TYPE_BOOL => wire::FieldKind::RepeatedBool,
            Type::TYPE_STRING | Type::TYPE_BYTES => wire::FieldKind::RepeatedBytes,
            Type::TYPE_MESSAGE => wire::FieldKind::RepeatedMessage,
            Type::TYPE_GROUP => wire::FieldKind::RepeatedGroup,
            Type::TYPE_ENUM => wire::FieldKind::RepeatedInt32,
        }
    } else {
        match field.r#type().unwrap() {
            Type::TYPE_INT32 => wire::FieldKind::Int32,
            Type::TYPE_UINT32 => wire::FieldKind::Varint32,
            Type::TYPE_INT64 | Type::TYPE_UINT64 => wire::FieldKind::Varint64,
            Type::TYPE_SINT32 => wire::FieldKind::Varint32Zigzag,
            Type::TYPE_SINT64 => wire::FieldKind::Varint64Zigzag,
            Type::TYPE_FIXED32 | Type::TYPE_SFIXED32 | Type::TYPE_FLOAT => wire::FieldKind::Fixed32,
            Type::TYPE_FIXED64 | Type::TYPE_SFIXED64 | Type::TYPE_DOUBLE => {
                wire::FieldKind::Fixed64
            }
            Type::TYPE_BOOL => wire::FieldKind::Bool,
            Type::TYPE_STRING | Type::TYPE_BYTES => wire::FieldKind::Bytes,
            Type::TYPE_MESSAGE => wire::FieldKind::Message,
            Type::TYPE_GROUP => wire::FieldKind::Group,
            Type::TYPE_ENUM => wire::FieldKind::Int32,
        }
    }
}

pub fn calculate_tag_with_syntax(field: &FieldDescriptorProto, syntax: Option<&str>) -> u32 {
    let is_repeated = field.label().unwrap() == Label::LABEL_REPEATED;

    // Determine if packed encoding should be used (only for repeated primitive fields)
    let is_packed = is_repeated
        && if let Some(opts) = field.options() {
            if opts.has_packed() {
                // Explicit packed option takes precedence
                opts.packed()
            } else {
                // No explicit option - use default based on syntax
                // Proto3 defaults to packed for repeated primitive fields
                // Proto2 defaults to unpacked
                syntax == Some("proto3")
            }
        } else {
            // No options at all - use syntax-based default
            syntax == Some("proto3")
        };

    let wire_type = match field.r#type().unwrap() {
        Type::TYPE_INT32
        | Type::TYPE_INT64
        | Type::TYPE_UINT32
        | Type::TYPE_UINT64
        | Type::TYPE_SINT32
        | Type::TYPE_SINT64
        | Type::TYPE_BOOL
        | Type::TYPE_ENUM => {
            if is_packed { 2 } else { 0 } // Packed encoding for repeated primitives
        }
        Type::TYPE_FIXED64 | Type::TYPE_SFIXED64 | Type::TYPE_DOUBLE => {
            if is_packed { 2 } else { 1 } // Packed encoding for repeated fixed64
        }
        Type::TYPE_STRING | Type::TYPE_BYTES | Type::TYPE_MESSAGE => 2,
        Type::TYPE_GROUP => 3,
        Type::TYPE_FIXED32 | Type::TYPE_SFIXED32 | Type::TYPE_FLOAT => {
            if is_packed { 2 } else { 5 } // Packed encoding for repeated fixed32
        }
    };

    (field.number() as u32) << 3 | wire_type
}

pub fn is_repeated(field: &FieldDescriptorProto) -> bool {
    field.label().unwrap() == Label::LABEL_REPEATED
}

pub fn is_message(field: &FieldDescriptorProto) -> bool {
    matches!(
        field.r#type().unwrap(),
        Type::TYPE_MESSAGE | Type::TYPE_GROUP
    )
}

pub fn needs_has_bit(field: &FieldDescriptorProto) -> bool {
    !is_repeated(field) && !is_message(field)
}

pub fn default_value<'a>(field: &'a FieldDescriptorProto) -> Option<Value<'a, 'a>> {
    use Type::*;

    match field.r#type()? {
        TYPE_BOOL => Some(Value::Bool(false)),
        TYPE_INT32 | TYPE_UINT32 | TYPE_ENUM => Some(Value::Int32(0)),
        TYPE_INT64 | TYPE_UINT64 => Some(Value::Int64(0)),
        TYPE_SINT32 => Some(Value::Int32(0)),
        TYPE_SINT64 => Some(Value::Int64(0)),
        TYPE_FIXED32 | TYPE_SFIXED32 | TYPE_FLOAT => Some(Value::Int32(0)),
        TYPE_FIXED64 | TYPE_SFIXED64 | TYPE_DOUBLE => Some(Value::Int64(0)),
        TYPE_STRING => Some(Value::String("")),
        TYPE_BYTES => Some(Value::Bytes(&[])),
        TYPE_MESSAGE | TYPE_GROUP => None,
    }
}

pub fn debug_message<'msg, T: Protobuf>(
    msg: &'msg T,
    f: &mut core::fmt::Formatter<'_>,
) -> core::fmt::Result {
    #[allow(mutable_transmutes)]
    let dynamic_msg = crate::reflection::DynamicMessage::new(unsafe {
        std::mem::transmute::<&'msg T, &'msg mut T>(msg)
    });
    use core::fmt::Debug;
    dynamic_msg.fmt(f)
}

pub struct DescriptorPool<'alloc> {
    pub arena: Arena<'alloc>,
    tables: std::collections::HashMap<std::string::String, &'alloc mut Table>,
}

impl<'alloc> DescriptorPool<'alloc> {
    pub fn new(alloc: &'alloc dyn core::alloc::Allocator) -> Self {
        DescriptorPool {
            arena: Arena::new(alloc),
            tables: std::collections::HashMap::new(),
        }
    }

    /// Strip leading dot from type name (protobuf returns ".package.Type", we store "package.Type")
    fn normalize_type_name(type_name: &str) -> &str {
        type_name.strip_prefix('.').unwrap_or(type_name)
    }

    /// Add a FileDescriptorProto to the pool
    pub fn add_file(&mut self, file: &'alloc FileDescriptorProto) {
        let package = if file.has_package() {
            file.package()
        } else {
            ""
        };

        // First pass: build all tables (child table pointers may be null)
        for message in file.message_type() {
            let full_name = if package.is_empty() {
                message.name().to_string()
            } else {
                format!("{}.{}", package, message.name())
            };
            self.add_message(message, &full_name, file.get_syntax());
        }

        // Second pass: patch aux entries with correct child table pointers
        for message in file.message_type() {
            let full_name = if package.is_empty() {
                message.name().to_string()
            } else {
                format!("{}.{}", package, message.name())
            };
            self.patch_message_aux_entries(&full_name);
        }
    }

    fn add_message(
        &mut self,
        message: &'alloc DescriptorProto,
        full_name: &str,
        syntax: Option<&str>,
    ) {
        // Build table from descriptor
        let table = self.build_table_from_descriptor(message, syntax);
        self.tables.insert(full_name.to_string(), table);

        // Add nested types
        for nested in message.nested_type() {
            let nested_full_name = format!("{}.{}", full_name, nested.name());
            self.add_message(nested, &nested_full_name, syntax);
        }
    }

    fn patch_message_aux_entries(&mut self, full_name: &str) {
        use crate::tables::AuxTableEntry;

        let table = match self.tables.get_mut(full_name) {
            Some(t) => &mut **t,
            None => return,
        };

        let descriptor = table.descriptor;

        // Count aux entries (message fields)
        let num_aux_entries = descriptor.field().iter().filter(|f| is_message(f)).count();
        if num_aux_entries == 0 {
            return;
        }

        // Get aux entry pointer - must use same Layout::extend logic as build_table_from_descriptor
        unsafe {
            // Recalculate aux offset using Layout::extend (accounts for padding)
            let table_layout = std::alloc::Layout::new::<Table>();
            let (_, aux_offset_from_table) = table_layout
                .extend(
                    std::alloc::Layout::array::<crate::decoding::TableEntry>(
                        table.num_decode_entries as usize,
                    )
                    .unwrap(),
                )
                .unwrap()
                .0
                .extend(std::alloc::Layout::array::<AuxTableEntry>(num_aux_entries).unwrap())
                .unwrap();

            let aux_ptr =
                (table as *mut Table as *mut u8).add(aux_offset_from_table) as *mut AuxTableEntry;

            // Patch each aux entry with the correct child table pointer
            let mut aux_idx = 0;
            for field in descriptor.field() {
                if is_message(field) {
                    let child_type_name = Self::normalize_type_name(field.type_name());
                    let child_table_ptr = self
                        .tables
                        .get_mut(child_type_name)
                        .map(|t| *t as *mut Table)
                        .unwrap_or(core::ptr::null_mut());

                    if !child_table_ptr.is_null() {
                        (*aux_ptr.add(aux_idx)).child_table = child_table_ptr;
                    }
                    aux_idx += 1;
                }
            }
        }

        // Patch nested types
        for nested in descriptor.nested_type() {
            let nested_full_name = format!("{}.{}", full_name, nested.name());
            self.patch_message_aux_entries(&nested_full_name);
        }
    }

    /// Get a table by message type name
    pub fn get_table(&self, message_type: &str) -> Option<&Table> {
        self.tables.get(message_type).map(|t| &**t)
    }

    pub fn create_message<'pool, 'msg>(
        &'pool self,
        message_type: &str,
        arena: &mut Arena<'msg>,
    ) -> anyhow::Result<DynamicMessage<'pool, 'msg>> {
        let table = &**self
            .tables
            .get(message_type)
            .ok_or_else(|| anyhow::anyhow!("Message type '{}' not found in pool", message_type))?;

        // Allocate object with proper alignment (8 bytes for all protobuf types)
        let layout = std::alloc::Layout::from_size_align(table.size as usize, 8)
            .map_err(|e| anyhow::anyhow!("Invalid layout: {}", e))?;
        let ptr = arena.alloc_raw(layout).as_ptr() as *mut Object;
        assert!((ptr as usize) & 7 == 0);
        let object = unsafe {
            // Zero-initialize the object
            core::ptr::write_bytes(ptr as *mut u8, 0, table.size as usize);
            &mut *ptr
        };

        Ok(DynamicMessage { object, table })
    }

    /// Create a DynamicMessage by decoding bytes with the given message type
    pub fn decode_message<'pool, 'msg>(
        &'pool self,
        message_type: &str,
        bytes: &[u8],
        arena: &'msg mut Arena,
    ) -> anyhow::Result<DynamicMessage<'pool, 'msg>> {
        let table = &**self
            .tables
            .get(message_type)
            .ok_or_else(|| anyhow::anyhow!("Message type '{}' not found in pool", message_type))?;

        // Allocate object with proper alignment (8 bytes for all protobuf types)
        let layout = std::alloc::Layout::from_size_align(table.size as usize, 8)
            .map_err(|e| anyhow::anyhow!("Invalid layout: {}", e))?;
        let ptr = arena.alloc_raw(layout).as_ptr() as *mut Object;
        assert!((ptr as usize) & 7 == 0);
        let object = unsafe {
            // Zero-initialize the object
            core::ptr::write_bytes(ptr as *mut u8, 0, table.size as usize);
            &mut *ptr
        };

        // Decode
        self.decode_into(object, table, bytes, arena)?;

        Ok(DynamicMessage { object, table })
    }

    // TODO: improve lifetime annotations
    #[allow(clippy::mut_from_ref)]
    fn build_table_from_descriptor(
        &mut self,
        descriptor: &'alloc DescriptorProto,
        syntax: Option<&str>,
    ) -> &'alloc mut Table {
        use crate::{decoding, encoding, tables::AuxTableEntry};

        // Calculate sizes
        let num_fields = descriptor.field().len();
        let num_has_bits = descriptor
            .field()
            .iter()
            .filter(|f| needs_has_bit(f))
            .count();
        let has_bits_size = (num_has_bits.div_ceil(32) * 4) as u32;

        // Calculate max field number for sparse decode table
        let max_field_number = descriptor
            .field()
            .iter()
            .map(|f| f.number())
            .max()
            .unwrap_or(0);

        if max_field_number > 2047 {
            panic!("Field numbers > 2047 not supported yet");
        }

        let num_decode_entries = (max_field_number + 1) as usize;

        // Calculate field offsets and total size using Layout::extend for proper padding
        // Start with has_bits layout (always u32 array, so alignment is 4)
        let mut layout = std::alloc::Layout::from_size_align(has_bits_size as usize, 4).unwrap();
        let mut field_offsets = std::vec::Vec::new();

        for &field in descriptor.field() {
            let field_size = self.field_size(field);
            let field_align = self.field_align(field);
            let field_layout =
                std::alloc::Layout::from_size_align(field_size as usize, field_align as usize)
                    .unwrap();

            let (new_layout, offset) = layout.extend(field_layout).unwrap();
            field_offsets.push((field, offset as u32));
            layout = new_layout;
        }

        // Pad to struct alignment
        let layout = layout.pad_to_align();
        let total_size = layout.size() as u32;

        // Count message fields for aux entries
        let num_aux_entries = descriptor.field().iter().filter(|f| is_message(f)).count();

        // Allocate table with entries - use Layout::extend to handle padding correctly
        let encode_layout = std::alloc::Layout::array::<encoding::TableEntry>(num_fields).unwrap();
        let (layout, table_offset) = encode_layout
            .extend(std::alloc::Layout::new::<Table>())
            .unwrap();
        let (layout, decode_offset) = layout
            .extend(std::alloc::Layout::array::<decoding::TableEntry>(num_decode_entries).unwrap())
            .unwrap();
        let (layout, aux_offset) = layout
            .extend(std::alloc::Layout::array::<AuxTableEntry>(num_aux_entries).unwrap())
            .unwrap();

        let base_ptr = self.arena.alloc_raw(layout).as_ptr();
        let encode_ptr = base_ptr as *mut encoding::TableEntry;
        let table_ptr = unsafe { base_ptr.add(table_offset) as *mut Table };
        let decode_ptr = unsafe { base_ptr.add(decode_offset) as *mut decoding::TableEntry };
        let aux_ptr = unsafe { base_ptr.add(aux_offset) as *mut AuxTableEntry };

        unsafe {
            // Initialize Table header
            (*table_ptr).num_encode_entries = num_fields as u16;
            (*table_ptr).num_decode_entries = num_decode_entries as u16;
            (*table_ptr).size = total_size as u16;
            // SAFETY: descriptor lives in arena with 'alloc lifetime, which outlives the table usage
            (*table_ptr).descriptor = core::mem::transmute::<
                &'alloc DescriptorProto,
                &'static DescriptorProto,
            >(descriptor);

            // Build aux index map for message fields and has_bit index map
            let mut aux_index_map = std::collections::HashMap::<i32, usize>::new();
            let mut has_bit_index_map = std::collections::HashMap::<i32, u32>::new();
            let mut aux_idx = 0;
            let mut has_bit_idx = 0u32;
            for &field in descriptor.field() {
                if is_message(field) {
                    aux_index_map.insert(field.number(), aux_idx);
                    aux_idx += 1;
                }
                if needs_has_bit(field) {
                    has_bit_index_map.insert(field.number(), has_bit_idx);
                    has_bit_idx += 1;
                }
            }

            // Build encode entries
            let mut has_bit_idx = 0u8;
            for (i, &(field, offset)) in field_offsets.iter().enumerate() {
                let has_bit = if needs_has_bit(field) {
                    let bit = has_bit_idx;
                    has_bit_idx += 1;
                    bit
                } else {
                    0
                };

                let entry_offset = if is_message(field) {
                    // For message fields, offset points to aux entry
                    let aux_index = aux_index_map[&field.number()];
                    let aux_offset =
                        (aux_ptr as usize) + aux_index * core::mem::size_of::<AuxTableEntry>();
                    let table_addr = table_ptr as usize;
                    (aux_offset - table_addr) as u16
                } else {
                    offset as u16
                };

                encode_ptr.add(i).write(encoding::TableEntry {
                    has_bit,
                    kind: field_kind_tokens(&field),
                    offset: entry_offset,
                    encoded_tag: calculate_tag_with_syntax(field, syntax),
                });
            }

            // Build decode entries - sparse array indexed by field number
            for field_number in 0..=max_field_number {
                if let Some(field) = descriptor
                    .field()
                    .iter()
                    .find(|f| f.number() == field_number)
                {
                    let entry = if is_message(field) {
                        // For message fields, offset points to aux entry
                        let aux_index = aux_index_map[&field_number];
                        let aux_offset =
                            (aux_ptr as usize) + aux_index * core::mem::size_of::<AuxTableEntry>();
                        let table_addr = table_ptr as usize;
                        decoding::TableEntry::new(
                            field_kind_tokens(field),
                            0, // has_bit not used for message fields
                            aux_offset - table_addr,
                        )
                    } else {
                        let offset = field_offsets
                            .iter()
                            .find(|(f, _)| f.number() == field_number)
                            .map(|(_, o)| *o)
                            .unwrap_or(0);
                        let has_bit = if needs_has_bit(field) {
                            has_bit_index_map[&field_number]
                        } else {
                            0
                        };
                        decoding::TableEntry::new(
                            field_kind_tokens(field),
                            has_bit,
                            offset as usize,
                        )
                    };
                    decode_ptr.add(field_number as usize).write(entry);
                } else {
                    // Empty entry for unused field number
                    decode_ptr
                        .add(field_number as usize)
                        .write(decoding::TableEntry(0));
                }
            }

            // Build aux entries for message fields
            for (aux_index, &(field, offset)) in field_offsets
                .iter()
                .filter(|(f, _)| is_message(f))
                .enumerate()
            {
                let child_type_name = Self::normalize_type_name(field.type_name());
                let child_table_ptr = self
                    .tables
                    .get_mut(child_type_name)
                    .map(|t| *t as *mut Table)
                    .unwrap_or(core::ptr::null_mut());

                aux_ptr.add(aux_index).write(AuxTableEntry {
                    offset,
                    child_table: child_table_ptr,
                });
            }

            &mut *table_ptr
        }
    }

    fn field_size(&self, field: &FieldDescriptorProto) -> u32 {
        use Type::*;

        if is_repeated(field) {
            return core::mem::size_of::<crate::containers::RepeatedField<u8>>() as u32;
        }

        match field.r#type().unwrap() {
            TYPE_BOOL => 1,
            TYPE_INT32 | TYPE_UINT32 | TYPE_SINT32 | TYPE_FIXED32 | TYPE_SFIXED32 | TYPE_FLOAT
            | TYPE_ENUM => 4,
            TYPE_INT64 | TYPE_UINT64 | TYPE_SINT64 | TYPE_FIXED64 | TYPE_SFIXED64 | TYPE_DOUBLE => {
                8
            }
            TYPE_STRING | TYPE_BYTES => core::mem::size_of::<String>() as u32,
            TYPE_MESSAGE | TYPE_GROUP => core::mem::size_of::<Message>() as u32,
        }
    }

    fn field_align(&self, field: &FieldDescriptorProto) -> u32 {
        use Type::*;

        if is_repeated(field) {
            return core::mem::align_of::<crate::containers::RepeatedField<u8>>() as u32;
        }

        match field.r#type().unwrap() {
            TYPE_BOOL => 1,
            TYPE_INT32 | TYPE_UINT32 | TYPE_SINT32 | TYPE_FIXED32 | TYPE_SFIXED32 | TYPE_FLOAT
            | TYPE_ENUM => 4,
            TYPE_INT64 | TYPE_UINT64 | TYPE_SINT64 | TYPE_FIXED64 | TYPE_SFIXED64 | TYPE_DOUBLE => {
                8
            }
            TYPE_STRING | TYPE_BYTES => core::mem::align_of::<String>() as u32,
            TYPE_MESSAGE | TYPE_GROUP => core::mem::align_of::<Message>() as u32,
        }
    }

    fn decode_into(
        &self,
        object: &mut Object,
        table: &Table,
        bytes: &[u8],
        arena: &mut Arena,
    ) -> anyhow::Result<()> {
        use crate::decoding::ResumeableDecode;

        let mut decoder = ResumeableDecode::<32>::new_from_table(object, table, isize::MAX);
        if !decoder.resume(bytes, arena) {
            return Err(anyhow::anyhow!("Decode failed"));
        }
        if !decoder.finish(arena) {
            return Err(anyhow::anyhow!("Decode finish failed"));
        }
        Ok(())
    }
}

pub struct DynamicMessage<'pool, 'msg> {
    pub object: &'msg mut Object,
    pub table: &'pool Table,
}

impl<'pool, 'msg> ProtobufExt for DynamicMessage<'pool, 'msg> {
    fn table(&self) -> &'static Table {
        // SAFETY: table lives in 'pool which outlives 'static usage here
        unsafe { std::mem::transmute(self.table) }
    }

    fn descriptor(&self) -> &'static DescriptorProto {
        self.table.descriptor
    }

    fn as_object(&self) -> &Object {
        self.object
    }

    fn as_object_mut(&mut self) -> &mut Object {
        self.object
    }
}

impl<'pool, 'msg> core::fmt::Debug for DynamicMessage<'pool, 'msg> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut debug_struct = f.debug_struct(self.table.descriptor.name());
        for field in self.table.descriptor.field() {
            if let Some(value) = self.get_field(field) {
                debug_struct.field(field.name(), &value);
            }
        }
        debug_struct.finish()
    }
}

impl<'pool, 'msg> DynamicMessage<'pool, 'msg> {
    pub fn new<T>(msg: &'msg mut T) -> Self
    where
        T: crate::Protobuf,
    {
        DynamicMessage {
            object: msg.as_object_mut(),
            table: T::table(),
        }
    }

    pub fn table(&self) -> &Table {
        self.table
    }

    pub fn descriptor(&self) -> &'pool DescriptorProto {
        self.table.descriptor
    }

    pub fn find_field_descriptor(&self, field_name: &str) -> Option<&'pool FieldDescriptorProto> {
        self.table
            .descriptor
            .field()
            .iter()
            .find(|&&field| field.name() == field_name)
            .copied()
    }

    pub fn find_field_descriptor_by_number(
        &self,
        field_number: i32,
    ) -> Option<&'pool FieldDescriptorProto> {
        self.table
            .descriptor
            .field()
            .iter()
            .find(|&&field| field.number() == field_number)
            .copied()
    }

    pub fn get_field(&'msg self, field: &'pool FieldDescriptorProto) -> Option<Value<'pool, 'msg>> {
        let entry = self.table.entry(field.number() as u32).unwrap();
        if field.label().unwrap() == Label::LABEL_REPEATED {
            // Repeated field
            match field.r#type().unwrap() {
                Type::TYPE_INT32 | Type::TYPE_SINT32 | Type::TYPE_SFIXED32 | Type::TYPE_ENUM => {
                    let slice = self.object.get_slice::<i32>(entry.offset() as usize);
                    if !slice.is_empty() {
                        Some(Value::RepeatedInt32(slice))
                    } else {
                        None
                    }
                }
                Type::TYPE_INT64 | Type::TYPE_SINT64 | Type::TYPE_SFIXED64 => {
                    let slice = self.object.get_slice::<i64>(entry.offset() as usize);
                    if !slice.is_empty() {
                        Some(Value::RepeatedInt64(slice))
                    } else {
                        None
                    }
                }
                Type::TYPE_UINT32 | Type::TYPE_FIXED32 => {
                    let slice = self.object.get_slice::<u32>(entry.offset() as usize);
                    if !slice.is_empty() {
                        Some(Value::RepeatedUInt32(slice))
                    } else {
                        None
                    }
                }
                Type::TYPE_UINT64 | Type::TYPE_FIXED64 => {
                    let slice = self.object.get_slice::<u64>(entry.offset() as usize);
                    if !slice.is_empty() {
                        Some(Value::RepeatedUInt64(slice))
                    } else {
                        None
                    }
                }
                Type::TYPE_FLOAT => {
                    let slice = self.object.get_slice::<f32>(entry.offset() as usize);
                    if !slice.is_empty() {
                        Some(Value::RepeatedFloat(slice))
                    } else {
                        None
                    }
                }
                Type::TYPE_DOUBLE => {
                    let slice = self.object.get_slice::<f64>(entry.offset() as usize);
                    if !slice.is_empty() {
                        Some(Value::RepeatedDouble(slice))
                    } else {
                        None
                    }
                }
                Type::TYPE_BOOL => {
                    let slice = self.object.get_slice::<bool>(entry.offset() as usize);
                    if !slice.is_empty() {
                        Some(Value::RepeatedBool(slice))
                    } else {
                        None
                    }
                }
                Type::TYPE_STRING => {
                    let slice = self.object.get_slice::<String>(entry.offset() as usize);
                    if !slice.is_empty() {
                        Some(Value::RepeatedString(slice))
                    } else {
                        None
                    }
                }
                Type::TYPE_BYTES => {
                    let slice = self.object.get_slice::<Bytes>(entry.offset() as usize);
                    if !slice.is_empty() {
                        Some(Value::RepeatedBytes(slice))
                    } else {
                        None
                    }
                }
                Type::TYPE_MESSAGE | Type::TYPE_GROUP => {
                    let aux = self.table.aux_entry_decode(entry);
                    let slice = self.object.get_slice::<Message>(aux.offset as usize);
                    if slice.is_empty() {
                        return None;
                    }
                    let dynamic_array = DynamicMessageArray {
                        object: slice,
                        table: unsafe { &*aux.child_table },
                    };
                    Some(Value::RepeatedMessage(dynamic_array))
                }
            }
        } else {
            let value = match field.r#type().unwrap() {
                Type::TYPE_INT32 | Type::TYPE_SINT32 | Type::TYPE_SFIXED32 | Type::TYPE_ENUM => {
                    Value::Int32(self.object.get(entry.offset() as usize))
                }
                Type::TYPE_INT64 | Type::TYPE_SINT64 | Type::TYPE_SFIXED64 => {
                    Value::Int64(self.object.get(entry.offset() as usize))
                }
                Type::TYPE_UINT32 | Type::TYPE_FIXED32 => {
                    Value::UInt32(self.object.get(entry.offset() as usize))
                }
                Type::TYPE_UINT64 | Type::TYPE_FIXED64 => {
                    Value::UInt64(self.object.get(entry.offset() as usize))
                }
                Type::TYPE_FLOAT => Value::Float(self.object.get(entry.offset() as usize)),
                Type::TYPE_DOUBLE => Value::Double(self.object.get(entry.offset() as usize)),
                Type::TYPE_BOOL => Value::Bool(self.object.get(entry.offset() as usize)),
                Type::TYPE_STRING => Value::String(
                    self.object
                        .ref_at::<crate::containers::String>(entry.offset() as usize)
                        .as_str(),
                ),
                Type::TYPE_BYTES => {
                    Value::Bytes(self.object.get_slice::<u8>(entry.offset() as usize))
                }
                Type::TYPE_MESSAGE | Type::TYPE_GROUP => {
                    let aux_entry = self.table.aux_entry_decode(entry);
                    let offset = aux_entry.offset as usize;
                    let msg = self.object.get::<Message>(offset);
                    if msg.0.is_null() {
                        return None;
                    }
                    let dynamic_msg = DynamicMessage {
                        object: unsafe { &mut *msg.0 },
                        table: unsafe { &*aux_entry.child_table },
                    };
                    return Some(Value::Message(dynamic_msg));
                }
            };
            debug_assert!(needs_has_bit(field));
            if self.object.has_bit(entry.has_bit_idx() as u8) {
                Some(value)
            } else {
                None
            }
        }
    }
}

pub struct DynamicMessageArray<'pool, 'msg> {
    pub object: &'msg [Message],
    pub table: &'pool Table,
}

impl<'pool, 'msg> core::fmt::Debug for DynamicMessageArray<'pool, 'msg> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list()
            .entries(self.object.iter().map(|msg| DynamicMessage {
                object: unsafe { &mut *msg.0 },
                table: self.table,
            }))
            .finish()
    }
}

impl<'pool, 'msg> DynamicMessageArray<'pool, 'msg> {
    pub fn len(&self) -> usize {
        self.object.len()
    }

    pub fn is_empty(&self) -> bool {
        self.object.is_empty()
    }

    pub fn get(&self, index: usize) -> DynamicMessage<'pool, 'msg> {
        let obj = self.object[index];
        DynamicMessage {
            object: unsafe { &mut *obj.0 },
            table: self.table,
        }
    }

    pub fn iter<'a>(&'a self) -> DynamicMessageArrayIter<'pool, 'a> {
        DynamicMessageArrayIter {
            array: self,
            index: 0,
        }
    }
}

// Iterator struct
pub struct DynamicMessageArrayIter<'pool, 'msg> {
    array: &'msg DynamicMessageArray<'pool, 'msg>,
    index: usize,
}

impl<'pool, 'msg> Iterator for DynamicMessageArrayIter<'pool, 'msg> {
    type Item = DynamicMessage<'pool, 'msg>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.array.len() {
            let item = self.array.get(self.index);
            self.index += 1;
            Some(item)
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.array.len() - self.index;
        (remaining, Some(remaining))
    }
}

impl<'pool, 'msg> ExactSizeIterator for DynamicMessageArrayIter<'pool, 'msg> {
    fn len(&self) -> usize {
        self.array.len() - self.index
    }
}

// IntoIterator for easy use with for loops
impl<'pool, 'msg> IntoIterator for &'msg DynamicMessageArray<'pool, 'msg> {
    type Item = DynamicMessage<'pool, 'msg>;
    type IntoIter = DynamicMessageArrayIter<'pool, 'msg>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[derive(Debug)]
pub enum Value<'pool, 'msg> {
    Int32(i32),
    Int64(i64),
    UInt32(u32),
    UInt64(u64),
    Float(f32),
    Double(f64),
    Bool(bool),
    String(&'msg str),
    Bytes(&'msg [u8]),
    Message(DynamicMessage<'pool, 'msg>),
    RepeatedInt32(&'msg [i32]),
    RepeatedInt64(&'msg [i64]),
    RepeatedUInt32(&'msg [u32]),
    RepeatedUInt64(&'msg [u64]),
    RepeatedFloat(&'msg [f32]),
    RepeatedDouble(&'msg [f64]),
    RepeatedBool(&'msg [bool]),
    RepeatedString(&'msg [String]),
    RepeatedBytes(&'msg [Bytes]),
    RepeatedMessage(DynamicMessageArray<'pool, 'msg>),
}
