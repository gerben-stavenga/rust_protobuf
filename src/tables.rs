#[repr(C)]
pub struct Table {
    pub num_encode_entries: u16,
    pub num_decode_entries: u16,
    pub size: u16,
    pub descriptor: &'static crate::google::protobuf::DescriptorProto::ProtoType,
}

impl Table {
    pub(crate) fn decode_entries(&self) -> &[crate::decoding::TableEntry] {
        unsafe {
            let ptr = (self as *const Self).add(1) as *const crate::decoding::TableEntry;
            core::slice::from_raw_parts(ptr, self.num_decode_entries as usize)
        }
    }

    pub(crate) fn encode_entries(&self) -> &[crate::encoding::TableEntry] {
        unsafe {
            let ptr = (self as *const _ as *const crate::encoding::TableEntry)
                .sub(self.num_encode_entries as usize);
            core::slice::from_raw_parts(ptr, self.num_encode_entries as usize)
        }
    }

    pub(crate) fn aux_entry(&self, offset: usize) -> AuxTableEntry {
        unsafe {
            let ptr = (self as *const Self as *const u8).add(offset);
            *(ptr as *const AuxTableEntry)
        }
    }

    #[allow(clippy::self_named_constructors)]
    pub(crate) fn table(encode_entries: &[crate::encoding::TableEntry]) -> &Self {
        unsafe { &*(encode_entries.as_ptr_range().end as *const Table) }
    }
}

#[repr(C)]
pub struct TableWithEntries<const E: usize, const D: usize, const A: usize> {
    pub encode_entries: [crate::encoding::TableEntry; E],
    pub table: Table,
    pub decode_entries: [crate::decoding::TableEntry; D],
    pub aux_entries: [AuxTableEntry; A],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AuxTableEntry {
    pub offset: u32,
    pub child_table: *const Table,
}

unsafe impl Send for AuxTableEntry {}
unsafe impl Sync for AuxTableEntry {}
