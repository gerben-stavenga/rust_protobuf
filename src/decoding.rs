use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;

use crate::base::Base;
use crate::repeated_field::{Bytes, RepeatedField};
use crate::wire::{ReadCursor, SLOP_SIZE, zigzag_decode};

struct TableEntry {
    has_bit: u8,
    kind: FieldKind,
    offset: u16,
}

struct AuxTableEntry {
    offset: u32,
    child_table: *const Table,
}

unsafe impl Send for AuxTableEntry {}
unsafe impl Sync for AuxTableEntry {}

pub struct Table {
    num_entries: u32,
    size: u32,
    create_fn: fn() -> &'static mut Base,
}

impl Table {
    fn entry(&self, field_number: u32) -> Option<&TableEntry> {
        if field_number >= self.num_entries {
            return None;
        }
        unsafe {
            let entries_ptr = (self as *const Table).add(1) as *const TableEntry;
            Some(&*entries_ptr.add(field_number as usize))
        }
    }

    fn aux_entry(&self, offset: u32) -> &AuxTableEntry {
        unsafe { 
            let aux_table_ptr = (self as *const Table as *const u8).add(offset as usize) as *const AuxTableEntry;
            &*aux_table_ptr
        }
    }
}

#[derive(Default, Clone, Copy)]
struct StackEntry {
    obj: *mut Base,
    table: *const Table,
    delta_limit_or_group_tag: isize,
}

#[repr(C)]
struct ParseContextHeader {
    obj: *mut Base,
    table: *const Table,
    limit: isize,
    depth: usize,
}

#[repr(C)]
struct ParseContext {
    header: ParseContextHeader,
    stack: [StackEntry],
}

impl ParseContext {
    fn push_limit(&mut self, ptr: ReadCursor, len: isize, end: NonNull<u8>, obj: &mut Base, table: &Table) -> Option<NonNull<u8>> {
        let new_limit = ptr - end + len;
        let delta_limit = self.header.limit - new_limit;
        if delta_limit < 0 {
            return None;
        }
        let depth = self.header.depth;
        if depth == 0 {
            return None;
        }
        let depth = depth - 1;
        self.header.depth = depth;
        self.stack[depth] = StackEntry {
            obj,
            table,
            delta_limit_or_group_tag: delta_limit,
        };
        self.header.limit = new_limit;
        Some(unsafe { end.offset(new_limit.min(0)) })
    }

    fn pop_limit<'a>(&mut self) -> Option<(&'a mut Base, &'a Table)> {
        let depth = self.header.depth;
        if depth == self.stack.len() {
            return None;
        }
        self.header.depth = depth + 1;
        let StackEntry { obj, table, delta_limit_or_group_tag } = self.stack[depth];
        self.header.limit += delta_limit_or_group_tag;
        unsafe { Some((&mut *obj, &*table)) }
    }

    fn push_group(&mut self, field_number: u32, obj: &mut Base, table: &Table) -> Option<()> {
        let depth = self.header.depth;
        if depth == 0 {
            return None;
        }
        let depth = depth - 1;
        self.header.depth = depth;
        self.stack[depth] = StackEntry {
            obj,
            table,
            delta_limit_or_group_tag: -(field_number as isize),
        };
        Some(())
    }

    fn pop_group<'a>(&mut self, field_number: u32) -> Option<(&'a mut Base, &'a Table)> {
        let depth = self.header.depth;
        if depth == self.stack.len() {
            return None;
        }
        self.header.depth = depth + 1;
        let StackEntry { obj, table, delta_limit_or_group_tag } = self.stack[depth];
        if field_number != -delta_limit_or_group_tag as u32 {
            return None;
        }
        unsafe { Some((&mut *obj, &*table)) }
    }
}

impl Base {
    fn get_or_create_child_object<'a>(&mut self, aux_entry: &AuxTableEntry, has_bit_idx: u32) -> (&'a mut Base, &'a Table) {
        let field = self.ref_mut::<*mut Base>(aux_entry.offset);
        let child_table = unsafe { &*aux_entry.child_table };
        let child = if (*field).is_null() {
            let child = (child_table.create_fn)();
            *field = child;
            self.set_has_bit(has_bit_idx);
            child
        } else {
            unsafe { &mut **field }
        };
        (child, child_table)
    }

    fn add_child_object<'a>(&mut self, aux_entry: &AuxTableEntry) -> (&'a mut Base, &'a Table) {
        let field = self.ref_mut::<RepeatedField<*mut Base>>(aux_entry.offset);
        let child_table = unsafe { &*aux_entry.child_table };
        let child = (child_table.create_fn)();
        field.push(child);
        (child, child_table)
    }
}


#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FieldKind {
    Unknown,
    Varint64,
    Varint32,
    Varint64Zigzag,
    Varint32Zigzag,
    Fixed64,
    Fixed32,
    Bytes,
    Message,
    Group,
    RepeatedVarint64,
    RepeatedVarint32,
    RepeatedVarint64Zigzag,
    RepeatedVarint32Zigzag,
    RepeatedFixed64,
    RepeatedFixed32,
    RepeatedBytes,
    RepeatedMessage,
    RepeatedGroup,
}

fn validate_wire_type(tag: u32, expected_wire_type: u8) -> Option<()> {
    if (tag & 7) == expected_wire_type as u32 {
        Some(())
    } else {
        None
    }
}

fn parse_loop(mut ptr: ReadCursor, end: NonNull<u8>, ctx: &mut ParseContext) -> Option<ReadCursor> {
    let mut table = unsafe { &*ctx.header.table };
    let mut obj = if table as *const Table as usize == 1 {
        // obj is a bytes field being parsed in chunks
        let bytes = unsafe { &mut *(ctx.header.obj as *mut Bytes) };
        if ctx.header.limit > SLOP_SIZE as isize {
            bytes.append(ptr.read_slice(SLOP_SIZE as isize - (ptr - end)));
            return Some(ptr);
        }
        bytes.append(ptr.read_slice(ctx.header.limit - (ptr - end)));
        let (obj, t) = ctx.pop_limit()?;
        table = t;
        obj
    } else {
        unsafe { &mut *ctx.header.obj }
    };
    loop {
        let mut limited_end = unsafe { end.offset(ctx.header.limit.min(0)) };
        while ptr < limited_end {
            let tag = ptr.read_tag()?;
            if tag == 0 {
                return Some(ptr);
            }
            let field_number = tag >> 3;
            if (tag & 7) == 4 {
                (obj, table) = ctx.pop_group(field_number)?;
                continue;
            }
            let entry = table.entry(field_number)?;
            let offset = entry.offset as u32;
            let has_bit_idx = entry.has_bit as u32;
            match entry.kind {
                FieldKind::Unknown => {
                    return None;
                }
                FieldKind::Varint64 => { // varint64
                    validate_wire_type(tag, 0);
                    let value = ptr.read_varint()?;
                    obj.set(offset, has_bit_idx, value);
                }
                FieldKind::Varint32 => { // varint32
                    validate_wire_type(tag, 0);
                    let value = ptr.read_varint()?;
                    obj.set(offset, has_bit_idx, value as u32);
                }
                FieldKind::Varint64Zigzag => { // varint64 zigzag
                    validate_wire_type(tag, 0);
                    let value = ptr.read_varint()?;
                    obj.set(offset, has_bit_idx, zigzag_decode(value));
                }
                FieldKind::Varint32Zigzag => { // varint32 zigzag
                    validate_wire_type(tag, 0);
                    let value = ptr.read_varint()?;
                    obj.set(offset, has_bit_idx, zigzag_decode(value) as u32);
                }
                FieldKind::Fixed64 => { // fixed64
                    validate_wire_type(tag, 1);
                    let value = ptr.read_unaligned::<u64>();
                    obj.set(offset, has_bit_idx, value);
                }
                FieldKind::Fixed32 => { // fixed32
                    validate_wire_type(tag, 5);
                    let value = ptr.read_unaligned::<u32>();
                    obj.set(offset, has_bit_idx, value);
                }
                FieldKind::Bytes => { // bytes
                    validate_wire_type(tag, 2);
                    let len = ptr.read_size()?;
                    if ptr - limited_end + len <= SLOP_SIZE as isize {
                        obj.set_bytes(offset, has_bit_idx, ptr.read_slice(len));
                    } else {
                        ctx.push_limit(ptr, len, end, obj, table)?;
                        ctx.header.obj = obj.set_bytes(offset, has_bit_idx, ptr.read_slice(SLOP_SIZE as isize - (ptr - end))) as *mut _ as *mut Base;
                        ctx.header.table = 1 as *const Table;
                        return Some(ptr);
                    }
                }
                FieldKind::Message => { // message
                    validate_wire_type(tag, 2);
                    let len = ptr.read_size()?;
                    let aux_entry = table.aux_entry(offset);
                    let (child, child_table) = obj.get_or_create_child_object(aux_entry, has_bit_idx);
                    limited_end = ctx.push_limit(ptr, len, end, obj, table)?;
                    (obj, table) = (child, child_table);
                }
                FieldKind::Group => { // start group
                    validate_wire_type(tag, 3);
                    ctx.push_group(field_number, obj, table)?;
                    (obj, table) = obj.get_or_create_child_object(table.aux_entry(offset, ), has_bit_idx);
                }
                FieldKind::RepeatedVarint64 => { // varint64
                    validate_wire_type(tag, 0);
                    let value = ptr.read_varint()?;
                    obj.add(offset, value);
                }
                FieldKind::RepeatedVarint32 => { // varint32
                    validate_wire_type(tag, 0);
                    let value = ptr.read_varint()?;
                    obj.add(offset, value as u32);
                }
                FieldKind::RepeatedVarint64Zigzag => { // varint64 zigzag
                    validate_wire_type(tag, 0);
                    let value = ptr.read_varint()?;
                    obj.add(offset, zigzag_decode(value));
                }
                FieldKind::RepeatedVarint32Zigzag => { // varint32 zigzag
                    validate_wire_type(tag, 0);
                    let value = ptr.read_varint()?;
                    obj.add(offset, zigzag_decode(value) as u32);
                }
                FieldKind::RepeatedFixed64 => { // fixed64
                    validate_wire_type(tag, 1);
                    let value = ptr.read_unaligned::<u64>();
                    obj.add(offset, value);
                }
                FieldKind::RepeatedFixed32 => { // fixed32
                    validate_wire_type(tag, 5);
                    let value = ptr.read_unaligned::<u32>();
                    obj.add(offset, value);
                }
                FieldKind::RepeatedBytes => { // bytes
                    validate_wire_type(tag, 2);
                    let len = ptr.read_size()?;
                    if ptr - limited_end + len <= SLOP_SIZE as isize {
                        obj.add_bytes(offset, ptr.read_slice(len));
                    } else {
                        ctx.push_limit(ptr, len, end, obj, table)?;
                        ctx.header.obj = obj.add_bytes(offset, ptr.read_slice(SLOP_SIZE as isize - (ptr - end))) as *mut _ as *mut Base;
                        ctx.header.table = 1 as *const Table;
                        return Some(ptr);
                    }
                }
                FieldKind::RepeatedMessage => { // message
                    validate_wire_type(tag, 2);
                    let len = ptr.read_size()?;
                    let aux_entry = table.aux_entry(offset);
                    let (child, child_table) = obj.add_child_object(aux_entry);
                    limited_end = ctx.push_limit(ptr, len, end, obj, table)?;
                    (obj, table) = (child, child_table);
                }
                FieldKind::RepeatedGroup => { // start group
                    validate_wire_type(tag, 3);
                    ctx.push_group(field_number, obj, table)?;
                    (obj, table) = obj.add_child_object(table.aux_entry(offset));
                }
            }
        }
        if ptr - end == ctx.header.limit {
            if ctx.header.depth == ctx.stack.len() {
                return Some(ptr);
            }
            (obj, table) = ctx.pop_limit()?;
            continue;
        }
        if ptr >= end {
            break;
        }
        if ptr != limited_end {
            return None;
        }
    }
    ctx.header.obj = obj;
    ctx.header.table = table;
    Some(ptr)
}

#[must_use]
pub fn parse_flat<const STACK_DEPTH: usize, T>(obj: &mut T, table: &Table, buf: &[u8]) -> bool {
    let mut parser = ResumeableParseWithStack::<STACK_DEPTH>::new(obj, table, isize::MAX);
    if !parser.resume(buf) {
        return false;
    }
    parser.finish()
}

pub fn parse_from_bufread<const STACK_DEPTH: usize, T>(obj: &mut T, table: &Table, reader: &mut impl std::io::BufRead) -> anyhow::Result<()> {
    let mut parser = ResumeableParseWithStack::<STACK_DEPTH>::new(obj, table, isize::MAX);
    let mut len = 0;
    loop {
        reader.consume(len);
        let buffer = reader.fill_buf()?;
        len = buffer.len();
        if len == 0 {
            break;
        }
        if !parser.resume(&buffer) {
            return Err(anyhow::anyhow!("parse error"));
        }
    }
    if !parser.finish() {
        return Err(anyhow::anyhow!("parse error"));
    }
    Ok(())
}

pub fn parse_from_read<const STACK_DEPTH: usize, T>(obj: &mut T, table: &Table, reader: &mut impl std::io::Read) -> anyhow::Result<()> {
    let mut buf_reader = std::io::BufReader::new(reader);
    parse_from_bufread::<STACK_DEPTH, _>(obj, table, &mut buf_reader)
}

#[repr(C)]
pub struct ResumeableParse {
    overrun: isize,
    patch_buffer: [u8; SLOP_SIZE * 2],
    ctx: ParseContext,
}

impl ResumeableParse {
    #[must_use]
    pub fn resume(&mut self, buf: &[u8]) -> bool {
        self.resume_impl(buf).is_some()
    }

    #[must_use]
    pub fn finish(&mut self) -> bool {
        if Self::go_parse(self.overrun, &self.patch_buffer[..SLOP_SIZE], &mut self.ctx) != Some(0) {
            return false;
        }
        self.ctx.header.depth == self.ctx.stack.len()
    }

    fn resume_impl(&mut self, buf: &[u8]) -> Option<()> {
        let size = buf.len();
        if buf.len() > SLOP_SIZE {
            self.patch_buffer[SLOP_SIZE..].copy_from_slice(&buf[..SLOP_SIZE]);
            let overrun = Self::go_parse(self.overrun, &self.patch_buffer[..SLOP_SIZE], &mut self.ctx)?;
            self.overrun = Self::go_parse(overrun, &buf[..size - SLOP_SIZE], &mut self.ctx)?;
            self.patch_buffer[..SLOP_SIZE].copy_from_slice(&buf[size - SLOP_SIZE..]);
        } else {
            self.patch_buffer[SLOP_SIZE..SLOP_SIZE + size].copy_from_slice(buf);
            self.overrun = Self::go_parse(self.overrun, &self.patch_buffer[..size], &mut self.ctx)?;
            self.patch_buffer.copy_within(size..size + SLOP_SIZE, 0);
        }
        Some(())
    }

    fn go_parse(overrun: isize, buf: &[u8], ctx: &mut ParseContext) -> Option<isize> {
        ctx.header.limit -= buf.len() as isize;
        if overrun < buf.len() as isize {
            let (start, end) = ReadCursor::new(&buf[..buf.len()]);
            let overrun = parse_loop(start + overrun, end, ctx)? - end;
            assert!(overrun >= 0 && overrun <= SLOP_SIZE as isize);
            Some(overrun)
        } else {
            Some(overrun - buf.len() as isize)
        }
    }
}

#[repr(C)]
struct ResumeableParseWithStack<const STACK_DEPTH: usize> {
    overrun: isize,
    patch_buffer: [u8; SLOP_SIZE * 2],
    header: ParseContextHeader,
    stack: [StackEntry; STACK_DEPTH],
}

impl<const STACK_DEPTH: usize> ResumeableParseWithStack<STACK_DEPTH> {
    pub fn new<T>(obj: &mut T, table: &Table, limit: isize) -> Self {
        Self {
            overrun: SLOP_SIZE as isize,
            patch_buffer: [0u8; SLOP_SIZE * 2],
            header: ParseContextHeader {
                obj: obj as *mut T as *mut Base,
                table,
                limit,
                depth: STACK_DEPTH,
            },
            stack: [StackEntry::default(); STACK_DEPTH],
        }
    }   
}

impl<const STACK_DEPTH: usize> Deref for ResumeableParseWithStack<STACK_DEPTH> {
    type Target = ResumeableParse;
    fn deref(&self) -> &Self::Target {
        unsafe { 
            let fat_p = std::ptr::slice_from_raw_parts(self, STACK_DEPTH) as *mut Self::Target;
            &mut *fat_p
        }
    }
}

impl<const STACK_DEPTH: usize> DerefMut for ResumeableParseWithStack<STACK_DEPTH> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { 
            let fat_p = std::ptr::slice_from_raw_parts_mut(self, STACK_DEPTH) as *mut Self::Target;
            &mut *fat_p
        }
    }
}


#[cfg(test)]
mod tests {
    use std::mem::offset_of;

    use super::*;


    #[repr(C)]
    #[derive(Debug, Default)]
    struct Test {
        cached_size: u32,
        has_bits: u32,  // Has to be at offset 0
        x: i32,
        y: u64,
        z: Bytes,
        child: *mut Test,
    }

    fn create() -> &'static mut Base {
        unsafe { &mut *(Box::into_raw(Box::new(Test::default())) as *mut Base) }
    }

    static TEST_TABLE: (Table, [TableEntry; 5], [AuxTableEntry; 1]) = (
        Table {
            num_entries: 5,
            size: std::mem::size_of::<Test>() as u32,
            create_fn: create,
        },
        [
            TableEntry {has_bit: 0, kind: FieldKind::Unknown, offset: offset_of!(Test, x) as u16},  // Placeholder for field number 0
            TableEntry {has_bit: 0, kind: FieldKind::Varint32, offset: offset_of!(Test, x) as u16},
            TableEntry {has_bit: 1, kind: FieldKind::Fixed64, offset: offset_of!(Test, y) as u16},
            TableEntry {has_bit: 2, kind: FieldKind::Bytes, offset: offset_of!(Test, z) as u16},
            TableEntry {has_bit: 3, kind: FieldKind::Message, offset: offset_of!((Table, [TableEntry; 5], [&'static Table; 1]), 2) as u16},
        ],
        [AuxTableEntry { offset: offset_of!(Test, child) as u32, child_table: &TEST_TABLE.0 }]
    );

    const BUFFER: [u8; 38] =  [
        // x varint 0
        0o10, 1,
        // y fixed 64, 2  
        0o21, 2, 0, 0, 0, 0, 0, 0, 0,  
        // z length delimted 11
        0o32, 21, b'H', b'e', b'l',  b'l', b'o', b' ', b'W', b'o', b'r', b'l', b'd', b'!', b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9',  
        // child is length delimited 34
        0o42, 2, 0o10, 2
    ];

    #[test]
    fn test_resumeable_parse() {
        let mut test = Test::default();

        assert!(parse_flat::<100, _>(&mut test, &TEST_TABLE.0, &BUFFER));

        println!("{:?} {:?}", &test, unsafe {
            &*test.child
        });
        std::mem::forget(test);
    }
}