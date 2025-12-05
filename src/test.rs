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
#[repr(C)]
#[derive(Debug, Default)]
pub struct Test {
    has_bits: [u32; 1],
    x: u32,
    y: u64,
    z: protobuf::containers::Bytes,
    child1: *mut protobuf::base::Object,
    child2: *mut protobuf::base::Object,
    nested_message: protobuf::containers::RepeatedField<*mut protobuf::base::Object>,
}

impl Test {
    pub fn x(&self) -> u32 {
        self.x
    }
    pub fn set_x(&mut self, value: u32) {
        self.as_object_mut().set_has_bit(0);
        self.x = value;
    }
    pub fn y(&self) -> u64 {
        self.y
    }
    pub fn set_y(&mut self, value: u64) {
        self.as_object_mut().set_has_bit(1);
        self.y = value;
    }
    pub fn z(&self) -> &[u8] {
        &self.z
    }
    pub fn set_z(&mut self, value: &[u8]) {
        self.as_object_mut().set_has_bit(2);
        self.z.assign(value);
    }
    pub fn child1(&self) -> Option<&Test> {
        if self.child1.is_null() {
            None
        } else {
            Some(unsafe { &*(self.child1 as *const Test) })
        }
    }
    pub fn child1_mut(&mut self) -> &mut Test {
        let object = self.child1;
        if object.is_null() {
            let new_object = protobuf::base::Object::create(std::mem::size_of::<Test>() as u32);
            self.child1 = new_object;
        }
        unsafe { &mut *(self.child1 as *mut Test) }
    }
    pub fn child2(&self) -> Option<&Test_Child2> {
        if self.child2.is_null() {
            None
        } else {
            Some(unsafe { &*(self.child2 as *const Test_Child2) })
        }
    }
    pub fn child2_mut(&mut self) -> &mut Test_Child2 {
        let object = self.child2;
        if object.is_null() {
            let new_object =
                protobuf::base::Object::create(std::mem::size_of::<Test_Child2>() as u32);
            self.child2 = new_object;
        }
        unsafe { &mut *(self.child2 as *mut Test_Child2) }
    }
    pub fn nested_message(&self) -> &[Test_NestedMessage] {
        unsafe { std::mem::transmute(self.nested_message.slice()) }
    }
    pub fn nested_message_mut(
        &mut self,
    ) -> &mut protobuf::containers::RepeatedField<*mut Test_NestedMessage> {
        unsafe { std::mem::transmute(&mut self.nested_message) }
    }
    // type Child2 = Test_Child2;
    // type NestedMessage = Test_NestedMessage;
}
impl protobuf::Protobuf for Test {
    fn encoding_table() -> &'static [crate::encoding::TableEntry] {
        &ENCODING_TABLE_Test.0
    }
    fn decoding_table() -> &'static crate::decoding::Table {
        &DECODING_TABLE_Test.0
    }
}

static DECODING_TABLE_Test: protobuf::decoding::TableWithEntries<7, 3> =
    protobuf::decoding::TableWithEntries(
        crate::decoding::Table {
            num_entries: 7,
            size: std::mem::size_of::<Test>() as u32,
        },
        [
            protobuf::decoding::TableEntry {
                has_bit: 0,
                kind: protobuf::wire::FieldKind::Unknown,
                offset: 0,
            },
            protobuf::decoding::TableEntry {
                has_bit: 0,
                kind: protobuf::wire::FieldKind::Varint32,
                offset: std::mem::offset_of!(Test, x) as u16,
            },
            protobuf::decoding::TableEntry {
                has_bit: 1,
                kind: protobuf::wire::FieldKind::Fixed64,
                offset: std::mem::offset_of!(Test, y) as u16,
            },
            protobuf::decoding::TableEntry {
                has_bit: 2,
                kind: protobuf::wire::FieldKind::Bytes,
                offset: std::mem::offset_of!(Test, z) as u16,
            },
            protobuf::decoding::TableEntry {
                has_bit: 0,
                kind: protobuf::wire::FieldKind::Message,
                offset: (std::mem::offset_of!(protobuf::decoding::TableWithEntries<7, 3>, 2)
                    + 0 * std::mem::size_of::<protobuf::decoding::AuxTableEntry>())
                    as u16,
            },
            protobuf::decoding::TableEntry {
                has_bit: 0,
                kind: protobuf::wire::FieldKind::Group,
                offset: (std::mem::offset_of!(protobuf::decoding::TableWithEntries<7, 3>, 2)
                    + 1 * std::mem::size_of::<protobuf::decoding::AuxTableEntry>())
                    as u16,
            },
            protobuf::decoding::TableEntry {
                has_bit: 0,
                kind: protobuf::wire::FieldKind::RepeatedMessage,
                offset: (std::mem::offset_of!(protobuf::decoding::TableWithEntries<7, 3>, 2)
                    + 2 * std::mem::size_of::<protobuf::decoding::AuxTableEntry>())
                    as u16,
            },
        ],
        [
            protobuf::decoding::AuxTableEntry {
                offset: std::mem::offset_of!(Test, child1) as u32,
                child_table: &DECODING_TABLE_Test.0,
            },
            protobuf::decoding::AuxTableEntry {
                offset: std::mem::offset_of!(Test, child2) as u32,
                child_table: &DECODING_TABLE_Test_Child2.0,
            },
            protobuf::decoding::AuxTableEntry {
                offset: std::mem::offset_of!(Test, nested_message) as u32,
                child_table: &DECODING_TABLE_Test_NestedMessage.0,
            },
        ],
    );

static ENCODING_TABLE_Test: protobuf::encoding::TableWithEntries<6, 3> =
    protobuf::encoding::TableWithEntries(
        [
            protobuf::encoding::TableEntry {
                has_bit: 0,
                kind: protobuf::wire::FieldKind::Varint32,
                offset: std::mem::offset_of!(Test, x) as u16,
                encoded_tag: 8,
            },
            protobuf::encoding::TableEntry {
                has_bit: 1,
                kind: protobuf::wire::FieldKind::Fixed64,
                offset: std::mem::offset_of!(Test, y) as u16,
                encoded_tag: 17,
            },
            protobuf::encoding::TableEntry {
                has_bit: 2,
                kind: protobuf::wire::FieldKind::Bytes,
                offset: std::mem::offset_of!(Test, z) as u16,
                encoded_tag: 26,
            },
            protobuf::encoding::TableEntry {
                has_bit: 0,
                kind: protobuf::wire::FieldKind::Message,
                offset: (std::mem::offset_of!(protobuf::encoding::TableWithEntries<6, 3>, 1)
                    + 0 * std::mem::size_of::<protobuf::encoding::AuxTableEntry>())
                    as u16,
                encoded_tag: 34,
            },
            protobuf::encoding::TableEntry {
                has_bit: 0,
                kind: protobuf::wire::FieldKind::Group,
                offset: (std::mem::offset_of!(protobuf::encoding::TableWithEntries<6, 3>, 1)
                    + 1 * std::mem::size_of::<protobuf::encoding::AuxTableEntry>())
                    as u16,
                encoded_tag: 43,
            },
            protobuf::encoding::TableEntry {
                has_bit: 0,
                kind: protobuf::wire::FieldKind::RepeatedMessage,
                offset: (std::mem::offset_of!(protobuf::encoding::TableWithEntries<6, 3>, 1)
                    + 2 * std::mem::size_of::<protobuf::encoding::AuxTableEntry>())
                    as u16,
                encoded_tag: 50,
            },
        ],
        [
            protobuf::encoding::AuxTableEntry {
                offset: std::mem::offset_of!(Test, child1),
                child_table: &ENCODING_TABLE_Test.0,
            },
            protobuf::encoding::AuxTableEntry {
                offset: std::mem::offset_of!(Test, child2),
                child_table: &ENCODING_TABLE_Test_Child2.0,
            },
            protobuf::encoding::AuxTableEntry {
                offset: std::mem::offset_of!(Test, nested_message),
                child_table: &ENCODING_TABLE_Test_NestedMessage.0,
            },
        ],
    );
#[repr(C)]
#[derive(Debug, Default)]
pub struct Test_Child2 {
    has_bits: [u32; 1],
    x: i64,
    recursive: *mut protobuf::base::Object,
}

impl Test_Child2 {
    pub fn x(&self) -> i64 {
        self.x
    }
    pub fn set_x(&mut self, value: i64) {
        self.as_object_mut().set_has_bit(0);
        self.x = value;
    }
    pub fn recursive(&self) -> Option<&Test> {
        if self.recursive.is_null() {
            None
        } else {
            Some(unsafe { &*(self.recursive as *const Test) })
        }
    }
    pub fn recursive_mut(&mut self) -> &mut Test {
        let object = self.recursive;
        if object.is_null() {
            let new_object = protobuf::base::Object::create(std::mem::size_of::<Test>() as u32);
            self.recursive = new_object;
        }
        unsafe { &mut *(self.recursive as *mut Test) }
    }
}
impl protobuf::Protobuf for Test_Child2 {
    fn encoding_table() -> &'static [crate::encoding::TableEntry] {
        &ENCODING_TABLE_Test_Child2.0
    }
    fn decoding_table() -> &'static crate::decoding::Table {
        &DECODING_TABLE_Test_Child2.0
    }
}

static DECODING_TABLE_Test_Child2: protobuf::decoding::TableWithEntries<3, 1> =
    protobuf::decoding::TableWithEntries(
        crate::decoding::Table {
            num_entries: 3,
            size: std::mem::size_of::<Test_Child2>() as u32,
        },
        [
            protobuf::decoding::TableEntry {
                has_bit: 0,
                kind: protobuf::wire::FieldKind::Unknown,
                offset: 0,
            },
            protobuf::decoding::TableEntry {
                has_bit: 0,
                kind: protobuf::wire::FieldKind::Varint64Zigzag,
                offset: std::mem::offset_of!(Test_Child2, x) as u16,
            },
            protobuf::decoding::TableEntry {
                has_bit: 0,
                kind: protobuf::wire::FieldKind::Message,
                offset: (std::mem::offset_of!(protobuf::decoding::TableWithEntries<3, 1>, 2)
                    + 0 * std::mem::size_of::<protobuf::decoding::AuxTableEntry>())
                    as u16,
            },
        ],
        [protobuf::decoding::AuxTableEntry {
            offset: std::mem::offset_of!(Test_Child2, recursive) as u32,
            child_table: &DECODING_TABLE_Test.0,
        }],
    );

static ENCODING_TABLE_Test_Child2: protobuf::encoding::TableWithEntries<2, 1> =
    protobuf::encoding::TableWithEntries(
        [
            protobuf::encoding::TableEntry {
                has_bit: 0,
                kind: protobuf::wire::FieldKind::Varint64Zigzag,
                offset: std::mem::offset_of!(Test_Child2, x) as u16,
                encoded_tag: 8,
            },
            protobuf::encoding::TableEntry {
                has_bit: 0,
                kind: protobuf::wire::FieldKind::Message,
                offset: (std::mem::offset_of!(protobuf::encoding::TableWithEntries<2, 1>, 1)
                    + 0 * std::mem::size_of::<protobuf::encoding::AuxTableEntry>())
                    as u16,
                encoded_tag: 18,
            },
        ],
        [protobuf::encoding::AuxTableEntry {
            offset: std::mem::offset_of!(Test_Child2, recursive),
            child_table: &ENCODING_TABLE_Test.0,
        }],
    );
#[repr(C)]
#[derive(Debug, Default)]
pub struct Test_NestedMessage {
    has_bits: [u32; 1],
    x: i64,
    recursive: *mut protobuf::base::Object,
}

impl Test_NestedMessage {
    pub fn x(&self) -> i64 {
        self.x
    }
    pub fn set_x(&mut self, value: i64) {
        self.as_object_mut().set_has_bit(0);
        self.x = value;
    }
    pub fn recursive(&self) -> Option<&Test> {
        if self.recursive.is_null() {
            None
        } else {
            Some(unsafe { &*(self.recursive as *const Test) })
        }
    }
    pub fn recursive_mut(&mut self) -> &mut Test {
        let object = self.recursive;
        if object.is_null() {
            let new_object = protobuf::base::Object::create(std::mem::size_of::<Test>() as u32);
            self.recursive = new_object;
        }
        unsafe { &mut *(self.recursive as *mut Test) }
    }
}
impl protobuf::Protobuf for Test_NestedMessage {
    fn encoding_table() -> &'static [crate::encoding::TableEntry] {
        &ENCODING_TABLE_Test_NestedMessage.0
    }
    fn decoding_table() -> &'static crate::decoding::Table {
        &DECODING_TABLE_Test_NestedMessage.0
    }
}

static DECODING_TABLE_Test_NestedMessage: protobuf::decoding::TableWithEntries<3, 1> =
    protobuf::decoding::TableWithEntries(
        crate::decoding::Table {
            num_entries: 3,
            size: std::mem::size_of::<Test_NestedMessage>() as u32,
        },
        [
            protobuf::decoding::TableEntry {
                has_bit: 0,
                kind: protobuf::wire::FieldKind::Unknown,
                offset: 0,
            },
            protobuf::decoding::TableEntry {
                has_bit: 0,
                kind: protobuf::wire::FieldKind::Varint64Zigzag,
                offset: std::mem::offset_of!(Test_NestedMessage, x) as u16,
            },
            protobuf::decoding::TableEntry {
                has_bit: 0,
                kind: protobuf::wire::FieldKind::Message,
                offset: (std::mem::offset_of!(protobuf::decoding::TableWithEntries<3, 1>, 2)
                    + 0 * std::mem::size_of::<protobuf::decoding::AuxTableEntry>())
                    as u16,
            },
        ],
        [protobuf::decoding::AuxTableEntry {
            offset: std::mem::offset_of!(Test_NestedMessage, recursive) as u32,
            child_table: &DECODING_TABLE_Test.0,
        }],
    );

static ENCODING_TABLE_Test_NestedMessage: protobuf::encoding::TableWithEntries<2, 1> =
    protobuf::encoding::TableWithEntries(
        [
            protobuf::encoding::TableEntry {
                has_bit: 0,
                kind: protobuf::wire::FieldKind::Varint64Zigzag,
                offset: std::mem::offset_of!(Test_NestedMessage, x) as u16,
                encoded_tag: 8,
            },
            protobuf::encoding::TableEntry {
                has_bit: 0,
                kind: protobuf::wire::FieldKind::Message,
                offset: (std::mem::offset_of!(protobuf::encoding::TableWithEntries<2, 1>, 1)
                    + 0 * std::mem::size_of::<protobuf::encoding::AuxTableEntry>())
                    as u16,
                encoded_tag: 18,
            },
        ],
        [protobuf::encoding::AuxTableEntry {
            offset: std::mem::offset_of!(Test_NestedMessage, recursive),
            child_table: &ENCODING_TABLE_Test.0,
        }],
    );
