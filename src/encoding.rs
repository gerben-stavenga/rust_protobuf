use std::ptr::NonNull;

use crate::{base::Base, decoding::{FieldKind}, repeated_field::{Bytes, RepeatedField}, wire::{SLOP_SIZE, WriteCursor, zigzag_encode}};

#[derive(Clone, Copy)]
pub struct TableEntry {
    has_bit: u8,
    kind: FieldKind,
    offset: u16,
    tag: [u8; 4],
}

#[derive(Clone, Copy)]
pub struct AuxTableEntry {
    offset: usize,
    table: *const [TableEntry],
}

fn aux_entry<'a>(offset: usize, table: *const [TableEntry]) -> (usize, &'a [TableEntry]) {
    unsafe {
        let thin_ptr = table as *const u8;
        let AuxTableEntry { offset, table } = *(thin_ptr.add(offset) as *const AuxTableEntry);
        (offset, &*table)
    }
}

struct StackEntry {
    obj: *const Base,
    table: *const [TableEntry],
    index: usize,
    byte_count: isize,
    tag: [u8; 4],
}

const STACK_DEPTH: usize = 32;

struct EncodeContext {
    obj: &'static Base,
    table: &'static [TableEntry],
    byte_count: isize,  // Relative to begin of buffer
    index: usize,
    depth: usize,
    stack: [StackEntry; STACK_DEPTH],
}

impl EncodeContext {
    fn byte_count(&self, ptr: WriteCursor, begin: NonNull<u8>) -> isize {
        self.byte_count - (ptr - begin)
    }

    fn push(&mut self, obj: &'static Base, table: &'static [TableEntry], index: usize, tag: [u8; 4], byte_count: isize) -> Option<()> {
        if self.depth == 0 {
            return None;
        }
        self.depth -= 1;
        self.stack[self.depth] = StackEntry {
            obj,
            table,
            index,
            tag,
            byte_count,
        };
        Some(())
    }

    fn pop(&mut self) -> Option<(&'static Base, &'static [TableEntry], usize, [u8; 4], isize)> {
        if self.depth == self.stack.len() {
            return None;
        }
        let entry = &self.stack[self.depth];
        self.depth += 1;
        Some((
            unsafe { &*entry.obj },
            unsafe { &*entry.table },
            entry.index,
            entry.tag,
            entry.byte_count,
        ))
    }
}

// Serialize backwards, so that length prefixes are easy to write.
fn encode_loop(mut ptr: WriteCursor, begin: NonNull<u8>, ctx: &mut EncodeContext) -> Option<WriteCursor> {
    let mut obj = ctx.obj;
    let mut table = ctx.table;
    let mut index = ctx.index;
    loop {
        while index >= table.len() {
            if ptr <= begin {
                break;
            }
            let Some((next_obj, next_table, next_index, tag, byte_count)) = ctx.pop() else {
                return Some(ptr);
            };
            if tag[3] & 7 == 2 {
                let field_byte_count = ctx.byte_count(ptr, begin) - byte_count;
                ptr.write_varint(field_byte_count as u64);
            }
            ptr.write_tag(tag);
            obj = next_obj;
            table = next_table;
            index = next_index;
        }
        let TableEntry { has_bit, kind, offset, tag } = table[index];
        let offset = offset as usize;
        match kind {
            FieldKind::Unknown => {
                unreachable!()
            }
            FieldKind::Varint64 =>
                if obj.has_bit(has_bit) {
                    if ptr <= begin {
                        break;
                    }
                    ptr.write_varint(obj.get::<u64>(offset));
                    ptr.write_tag(tag);
                },
            FieldKind::Varint32 => {
                if obj.has_bit(has_bit) {
                    if ptr <= begin {
                        break;
                    }
                    ptr.write_varint(obj.get::<u32>(offset) as u64);
                    ptr.write_tag(tag);
                }
            }
            FieldKind::Varint64Zigzag => {
                if obj.has_bit(has_bit) {
                    if ptr <= begin {
                        break;
                    }
                    ptr.write_varint(zigzag_encode(obj.get::<i64>(offset)));
                    ptr.write_tag(tag);
                }
            }
            FieldKind::Varint32Zigzag => {
                if obj.has_bit(has_bit) {
                    if ptr <= begin {
                        break;
                    }
                    ptr.write_varint(zigzag_encode(obj.get::<i32>(offset) as i64));
                    ptr.write_tag(tag);
                }
            }
            FieldKind::Fixed64 => {
                if obj.has_bit(has_bit) {
                    if ptr <= begin {
                        break;
                    }
                    ptr.write_unaligned(obj.get::<u64>(offset));
                    ptr.write_tag(tag);
                }
            }
            FieldKind::Fixed32 => {
                if obj.has_bit(has_bit) {
                    if ptr <= begin {
                        break;
                    }
                    ptr.write_unaligned(obj.get::<u32>(offset));
                    ptr.write_tag(tag);
                }
            }
            FieldKind::Bytes => {
                if obj.has_bit(has_bit) {
                    if ptr <= begin {
                        break;
                    }
                    let bytes = obj.ref_at::<RepeatedField<u8>>(offset).as_ref();
                    if ptr - begin + (bytes.len() as isize) > SLOP_SIZE as isize {
                        unimplemented!();
                    }
                    ptr.write_slice(bytes);
                    ptr.write_varint(bytes.len() as u64);
                    ptr.write_tag(tag);
                }
            }
            FieldKind::Message => {
                if obj.has_bit(has_bit) {
                    if ptr <= begin {
                        break;
                    }
                    let (offset, child_table) = aux_entry(offset, table);
                        aux_entry(offset, table);
                    let message = unsafe { &*obj.get::<*const Base>(offset) };
                    ctx.push(obj, table, index, tag, ctx.byte_count(ptr, begin))?;
                    obj = message;
                    table = child_table;
                    index = 0;
                }
            }
            FieldKind::Group => {
                if obj.has_bit(has_bit) {
                    if ptr <= begin {
                        break;
                    }
                    let (offset, child_table) = aux_entry(offset, table);
                    let mut end_tag = tag;
                    end_tag[3] += 1;  // Set wire type to END_GROUP
                    ptr.write_tag(end_tag);
                    let message = unsafe { &*obj.get::<*const Base>(offset) };
                    ctx.push(obj, table, index, tag, 0)?;
                    obj = message;
                    table = child_table;
                    index = 0;
                }
            }
            FieldKind::RepeatedVarint64 => {
                for &val in obj.ref_at::<RepeatedField<u64>>(offset).as_ref() {
                    if ptr <= begin {
                        break;
                    }
                    ptr.write_varint(val);
                    ptr.write_tag(tag);
                }
            }
            FieldKind::RepeatedVarint32 => {
                for &val in obj.ref_at::<RepeatedField<u32>>(offset).as_ref() {
                    if ptr <= begin {
                        break;
                    }
                    ptr.write_varint(val as u64);
                    ptr.write_tag(tag);
                }
            }
            FieldKind::RepeatedVarint64Zigzag => {
                for &val in obj.ref_at::<RepeatedField<i64>>(offset).as_ref() {
                    if ptr <= begin {
                        break;
                    }
                    ptr.write_varint(zigzag_encode(val));
                    ptr.write_tag(tag);
                }
            }
            FieldKind::RepeatedVarint32Zigzag => {
                for &val in obj.ref_at::<RepeatedField<i32>>(offset).as_ref() {
                    if ptr <= begin {
                        break;
                    }
                    ptr.write_varint(zigzag_encode(val as i64));
                    ptr.write_tag(tag);
                }
            }
            FieldKind::RepeatedFixed64 => {
                for &val in obj.ref_at::<RepeatedField<u64>>(offset).as_ref() {
                    if ptr <= begin {
                        break;
                    }
                    ptr.write_unaligned(val);
                    ptr.write_tag(tag);
                }
            }
            FieldKind::RepeatedFixed32 => {
                for &val in obj.ref_at::<RepeatedField<u32>>(offset).as_ref() {
                    if ptr <= begin {
                        break;
                    }
                    ptr.write_unaligned(val);
                    ptr.write_tag(tag);
                }
            }
            FieldKind::RepeatedBytes => {
                for bytes in obj.ref_at::<RepeatedField<Bytes>>(offset).as_ref() {
                    if ptr <= begin {
                        break;
                    }
                    if ptr - begin + (bytes.len() as isize) > SLOP_SIZE as isize {
                        unimplemented!();
                    }
                    ptr.write_slice(bytes);
                    ptr.write_varint(bytes.len() as u64);
                    ptr.write_tag(tag);
                }
            }
            FieldKind::RepeatedMessage => {
                let (offset, child_table) = aux_entry(offset, table);
                for &message_ptr in obj.ref_at::<RepeatedField<*const Base>>(offset).as_ref() {
                    if ptr <= begin {
                        break;
                    }
                    let message = unsafe { &*message_ptr };
                    ctx.push(obj, table, index, tag, ctx.byte_count(ptr, begin))?;
                    obj = message;
                    table = child_table;
                    index = 0;
                }
            }
            FieldKind::RepeatedGroup => {
                let (offset, child_table) = aux_entry(offset, table);
                for &message_ptr in obj.ref_at::<RepeatedField<*const Base>>(offset).as_ref() {
                    if ptr <= begin {
                        break;
                    }
                    let mut end_tag = tag;
                    end_tag[3] += 1;  // Set wire type to END_GROUP
                    ptr.write_tag(end_tag);
                    let message = unsafe { &*message_ptr };
                    ctx.push(obj, table, index, tag, 0)?;
                    obj = message;
                    table = child_table;
                    index = 0;
                }
            }
        }
        index += 1;
    }
    ctx.obj = obj;
    ctx.table = table;
    ctx.index = index;
    Some(ptr)
}

/* 
struct ResumeableEncode {
    overrun: isize,
    patch_buffer: [u8; 2 * SLOP_SIZE],
    ctx: EncodeContext,
}

impl ResumeableEncode {
    pub fn new(obj: &'static Base, table: &'static [TableEntry]) -> Self {
        Self {
            overrun: 0,
            patch_buffer: [0; 2 * SLOP_SIZE],
            ctx: EncodeContext {
                obj,
                table,
                byte_count: 0,
                index: 0,
                depth: STACK_DEPTH,
                stack: [StackEntry {
                    obj: std::ptr::null(),
                    table: std::ptr::null(),
                    index: 0,
                    tag: [0; 4],
                    byte_count: 0,
                }; STACK_DEPTH],
            },
        }
    }

    pub fn resume_encode(&mut self, buffer: &mut [u8]) -> bool {
        if buffer.len() > SLOP_SIZE {
            buffer[buffer.len() - SLOP_SIZE..].copy_from_slice(&self.patch_buffer[..SLOP_SIZE]);
            let (ptr, begin) = WriteCursor::new(&mut buffer[SLOP_SIZE..]);
            let Some(overrun) = go_encode(ptr, begin, &mut self.ctx) else {
                return true;
            };
        } else {
            self.patch_buffer.copy_within(0, dest);
        }
    }
}

pub fn encode_message<'a>(
    buffer: &'a mut [u8],
    obj: &'static Base,
    table: &'static [TableEntry],
) -> anyhow::Result<&'a mut [u8]> {
    let mut resumeable_encode = ResumeableEncode::new(obj, table);
    if resumeable_encode.resume_encode(buffer) {
        Ok(&mut buffer[SLOP_SIZE - resumeable_encode.overrun..])
    } else {
        Err(anyhow::anyhow!("Buffer too small for message"))
    }
}
*/