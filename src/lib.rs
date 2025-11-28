use std::ptr::NonNull;

use crate::repeated_field::{Bytes, RepeatedField};
use crate::wire::{zigzag_decode, ReadCursor};


pub mod repeated_field;
pub mod wire;

pub struct LocalCapture<'a, T> {
    value: std::mem::ManuallyDrop<T>,
    origin: &'a mut T,
}

impl<'a, T> LocalCapture<'a, T> {
    pub fn new(origin: &'a mut T) -> Self {
        Self { value: std::mem::ManuallyDrop::new(unsafe { std::ptr::read(origin) }), origin }
    }
}

impl<'a, T> std::ops::Deref for LocalCapture<'a, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<'a, T> std::ops::DerefMut for LocalCapture<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

impl<'a, T> Drop for LocalCapture<'a, T> {
    fn drop(&mut self) {
        unsafe {
            std::ptr::write(self.origin, std::mem::ManuallyDrop::take(&mut self.value));
        }
    }
}


const SLOP_SIZE: usize = 16;

struct TableEntry {
    has_bit: u8,
    kind: FieldKind,
    data_offset: u16,
}

struct AuxTableEntry {
    offset: u32,
    child_table: *const Table,
}

pub struct Table {
    num_entries: u32,
    size: u32,
    create_fn: fn() -> &'static mut Object,
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
    obj: *mut Object,
    table: *const Table,
    delta_limit_or_group_tag: isize,
}

const STACK_DEPTH: usize = 100;

struct ParseContext {
    obj: *mut Object,
    table: *const Table,
    limit: isize,
    depth: usize,
    stack: [StackEntry; STACK_DEPTH],
}

pub struct Object;

impl Object {
    fn set_has(&mut self, has_bit_idx: u32) {
        let has_bit_offset = has_bit_idx / 32;
        *self.ref_mut::<u32>(has_bit_offset) |= 1 << (has_bit_idx as usize % 32);
    }

    fn ref_mut<T>(&mut self, offset: u32) -> &mut T {
        unsafe { &mut *((self as *mut Object as *mut u8).add(offset as usize) as *mut T) }
    }

    fn add<T>(&mut self, offset: u32, val: T) {
        let field = self.ref_mut::<RepeatedField<T>>(offset);
        field.push(val);
    }

    fn get_or_create_child_object<'a>(&mut self, aux_entry: &AuxTableEntry, has_bit_idx: u32) -> (&'a mut Object, &'a Table) {
        let field = self.ref_mut::<*mut Object>(aux_entry.offset);
        let child_table = unsafe { &*aux_entry.child_table };
        let child = if (*field).is_null() {
            let child = (child_table.create_fn)();
            *field = child;
            self.set_has(has_bit_idx);
            child
        } else {
            unsafe { &mut **field }
        };
        (child, child_table)
    }

    fn add_child_object<'a>(&mut self, aux_entry: &AuxTableEntry) -> (&'a mut Object, &'a Table) {
        let field = self.ref_mut::<RepeatedField<*mut Object>>(aux_entry.offset);
        let child_table = unsafe { &*aux_entry.child_table };
        let child = (child_table.create_fn)();
        field.push(child);
        (child, child_table)
    }

    fn set_bytes(&mut self, offset: u32, has_bit_idx: u32, bytes: &[u8]) -> &mut Bytes {
        self.set_has(has_bit_idx);
        let field = self.ref_mut::<Bytes>(offset);
        field.assign(bytes);
        field
    }

    fn add_bytes(&mut self, offset: u32, bytes: &[u8]) -> &mut Bytes {
        let field = self.ref_mut::<RepeatedField<Bytes>>(offset);
        let b = Bytes::from_slice(bytes);
        field.push(b);
        field.last_mut().unwrap()
    }

}

impl ParseContext {
    fn push_limit(&mut self, ptr: ReadCursor, len: isize, end: NonNull<u8>, obj: &mut Object, table: &Table) -> Option<NonNull<u8>> {
        let new_limit = ptr - end + len;
        let delta_limit = self.limit - new_limit;
        if delta_limit < 0 {
            return None;
        }
        let depth = self.depth;
        if depth == 0 {
            return None;
        }
        let depth = depth - 1;
        self.depth = depth;
        self.stack[depth] = StackEntry {
            obj,
            table,
            delta_limit_or_group_tag: delta_limit,
        };
        self.limit = new_limit;
        Some(unsafe { end.offset(new_limit.min(0)) })
    }

    fn pop_limit<'a>(&mut self) -> Option<(&'a mut Object, &'a Table)> {
        let depth = self.depth;
        if depth == self.stack.len() {
            return None;
        }
        self.depth = depth + 1;
        let StackEntry { obj, table, delta_limit_or_group_tag } = self.stack[depth];
        self.limit += delta_limit_or_group_tag;
        unsafe { Some((&mut *obj, &*table)) }
    }

    fn push_group(&mut self, field_number: u32, obj: &mut Object, table: &Table) -> Option<()> {
        let depth = self.depth;
        if depth == 0 {
            return None;
        }
        let depth = depth - 1;
        self.depth = depth;
        self.stack[depth] = StackEntry {
            obj,
            table,
            delta_limit_or_group_tag: -(field_number as isize),
        };
        Some(())
    }

    fn pop_group<'a>(&mut self, field_number: u32) -> Option<(&'a mut Object, &'a Table)> {
        let depth = self.depth;
        if depth == self.stack.len() {
            return None;
        }
        self.depth = depth + 1;
        let StackEntry { obj, table, delta_limit_or_group_tag } = self.stack[depth];
        if field_number != -delta_limit_or_group_tag as u32 {
            return None;
        }
        unsafe { Some((&mut *obj, &*table)) }
    }
}


#[repr(u8)]
pub enum FieldKind {
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
    let mut obj = unsafe { &mut *ctx.obj };
    let mut table = unsafe { &*ctx.table };
    loop {
        let mut limited_end = unsafe { end.offset(ctx.limit.min(0)) };
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
            let offset = entry.data_offset as u32;
            let has_bit_idx = entry.has_bit as u32;
            match entry.kind {
                FieldKind::Varint64 => { // varint64
                    validate_wire_type(tag, 0);
                    let value = ptr.read_varint()?;
                    *obj.ref_mut(offset) = value;
                    obj.set_has(has_bit_idx);
                }
                FieldKind::Varint32 => { // varint32
                    validate_wire_type(tag, 0);
                    let value = ptr.read_varint()?;
                    *obj.ref_mut(offset) = value as u32;
                    obj.set_has(has_bit_idx);
                }
                FieldKind::Varint64Zigzag => { // varint64 zigzag
                    validate_wire_type(tag, 0);
                    let value = ptr.read_varint()?;
                    *obj.ref_mut(offset) = zigzag_decode(value);
                    obj.set_has(has_bit_idx);
                }
                FieldKind::Varint32Zigzag => { // varint32 zigzag
                    validate_wire_type(tag, 0);
                    let value = ptr.read_varint()?;
                    *obj.ref_mut(offset) = zigzag_decode(value) as u32;
                    obj.set_has(has_bit_idx);
                }
                FieldKind::Fixed64 => { // fixed64
                    validate_wire_type(tag, 1);
                    let value = ptr.read_unaligned::<u64>();
                    *obj.ref_mut(offset) = value;
                    obj.set_has(has_bit_idx);
                }
                FieldKind::Fixed32 => { // fixed32
                    validate_wire_type(tag, 5);
                    let value = ptr.read_unaligned::<u32>();
                    *obj.ref_mut(offset) = value;
                    obj.set_has(has_bit_idx);
                }
                FieldKind::Bytes => { // bytes
                    validate_wire_type(tag, 2);
                    let len = ptr.read_size()?;
                    if ptr - limited_end + len <= SLOP_SIZE as isize {
                        obj.set_bytes(offset, has_bit_idx, ptr.read_slice(len));
                    } else {
                        ctx.push_limit(ptr, len, end, obj, table)?;
                        ctx.obj = obj.set_bytes(offset, has_bit_idx, ptr.read_slice(SLOP_SIZE as isize - (ptr - end))) as *mut _ as *mut Object;
                        ctx.table = 1 as *const Table;
                        return Some(ptr);
                    }
                }
                FieldKind::Message => { // message
                    validate_wire_type(tag, 2);
                    let len = ptr.read_size()?;
                    let aux_entry = table.aux_entry(offset);
                    let (child, child_table) = obj.get_or_create_child_object(aux_entry, has_bit_idx);
                    if ptr - limited_end + len <= 0 {
                        let new_end = ptr.limit(len);
                        ptr = parse_loop_chunk(ptr, new_end, child, child_table, 0)?;
                    } else {
                        limited_end = ctx.push_limit(ptr, len, end, obj, table)?;
                        (obj, table) = (child, child_table);
                    }
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
                    obj.set_has(has_bit_idx);
                }
                FieldKind::RepeatedBytes => { // bytes
                    validate_wire_type(tag, 2);
                    let len = ptr.read_size()?;
                    if ptr - limited_end + len <= SLOP_SIZE as isize {
                        obj.add_bytes(offset, ptr.read_slice(len));
                    } else {
                        ctx.push_limit(ptr, len, end, obj, table)?;
                        ctx.obj = obj.add_bytes(offset, ptr.read_slice(SLOP_SIZE as isize - (ptr - end))) as *mut _ as *mut Object;
                        ctx.table = 1 as *const Table;
                        return Some(ptr);
                    }
                }
                FieldKind::RepeatedMessage => { // message
                    validate_wire_type(tag, 2);
                    let len = ptr.read_size()?;
                    let aux_entry = table.aux_entry(offset);
                    let (child, child_table) = obj.add_child_object(aux_entry);
                    if ptr - limited_end + len <= 0 {
                        let new_end = ptr.limit(len);
                        ptr = parse_loop_chunk(ptr, new_end, child, child_table, 0)?;
                    } else {
                        limited_end = ctx.push_limit(ptr, len, end, obj, table)?;
                        (obj, table) = (child, child_table);
                    }
                }
                FieldKind::RepeatedGroup => { // start group
                    validate_wire_type(tag, 3);
                    ctx.push_group(field_number, obj, table)?;
                    (obj, table) = obj.add_child_object(table.aux_entry(offset));
                }
            }
        }
        if ptr >= end {
            break;
        }
        if ptr != limited_end {
            return None;
        }
        (obj, table) = ctx.pop_limit()?;
    }
    Some(ptr)
}

pub fn parse_flat(obj: &mut Object, table: &Table, buf: &[u8]) -> Option<()> {
    let mut patch_buffer = [0u8; SLOP_SIZE * 2];
    if buf.len() <= SLOP_SIZE {
        patch_buffer[..buf.len()].copy_from_slice(buf);
        let (start, end) = ReadCursor::new(&patch_buffer[..buf.len()]);
        parse_loop_chunk(start, end, obj, table, 0)?;
        Some(())
    } else {
        let mut ctx = ParseContext {
            obj,
            table,
            limit: buf.len() as isize,
            depth: 100,
            stack: [Default::default(); 100],
        };
        let (start, end) = ReadCursor::new(&buf[..buf.len() - SLOP_SIZE]);
        let overrun = parse_loop(start, end, &mut ctx)? - end;
        assert!(overrun >= 0 && overrun <= SLOP_SIZE as isize);
        patch_buffer[..SLOP_SIZE].copy_from_slice(buf[buf.len() - SLOP_SIZE..].as_ref());
        let (start, end) = ReadCursor::new(&patch_buffer[..SLOP_SIZE]);
        if parse_loop(start + overrun, end, &mut ctx)? == end {
            Some(())
        } else {
            None
        }
    }
}

pub fn parse_from_bufread(obj: &mut Object, table: &Table, reader: &mut impl std::io::BufRead) -> Result<()> {
    let mut parser = ResumeableParse::new(obj, table, isize::MAX);
    loop {
        let buffer = reader.fill_buf()?;
        parser.resume(&buffer[..n]).ok_or(error!("parse error"))?;
        reader.consume(buffer.len());
    }
    parser.finish()
}

pub struct ResumeableParse {
    overrun: isize,
    patch_buffer: [u8; SLOP_SIZE * 2],
    ctx: ParseContext,
}

impl ResumeableParse {
    pub fn new(obj: &mut Object, table: &Table, limit: isize) -> Self {
        let patch_buffer = [0u8; SLOP_SIZE * 2];
        let ctx = ParseContext {
            obj,
            table,
            limit,
            depth: 100,
            stack: [Default::default(); 100],
        };
        Self {
            overrun: SLOP_SIZE as isize,
            patch_buffer,
            ctx,
        }
    }

    pub fn resume(&mut self, buf: &[u8]) -> Option<()> {
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

    pub fn finish(&mut self) -> Option<()> {
        let overrun = Self::go_parse(self.overrun, &self.patch_buffer[..SLOP_SIZE], &mut self.ctx)?;
        if overrun == 0 && self.ctx.depth == self.ctx.stack.len() {
            Some(())
        } else {
            None
        }
    }

    fn go_parse(overrun: isize, buf: &[u8], ctx: &mut ParseContext) -> Option<isize> {
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