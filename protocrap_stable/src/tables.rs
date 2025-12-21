


#[repr(C)]
pub struct Table {
    pub num_encode_entries: u16,
    pub num_decode_entries: u16,
    pub size: u16,
    pub descriptor: &'static crate::google::protobuf::DescriptorProto::ProtoType,
}


#[repr(C)]
pub struct TableWithEntries<const E: usize, const D: usize, const A: usize> {
    pub encode_entries: [crate::encoding::TableEntry; E],
    pub header: Table,
    pub decode_entries: [crate::decoding::TableEntry; D],
    pub aux_entries: [AuxTableEntry; A],
}