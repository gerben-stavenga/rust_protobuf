use std::mem::MaybeUninit;
use std::ptr::NonNull;

use crate::Protobuf;
use crate::base::Object;
use crate::containers::{Bytes, RepeatedField};
use crate::utils::{Stack, StackWithStorage};
use crate::wire::{FieldKind, ReadCursor, SLOP_SIZE, zigzag_decode};

#[repr(C)]
#[derive(Debug)]
pub struct TableEntry {
    pub kind: FieldKind,
    pub has_bit: u8,
    pub offset: u16,
}

// assert size of TableEntry is 4 bytes
const _: [u8; 4] = [0; std::mem::size_of::<TableEntry>()];

#[repr(C)]
pub struct AuxTableEntry {
    pub offset: u32,
    pub child_table: *const Table,
}

unsafe impl Send for AuxTableEntry {}
unsafe impl Sync for AuxTableEntry {}

#[repr(C)]
#[derive(Debug)]
pub struct Table {
    pub num_entries: u32,
    pub size: u32,
}

struct TableEntryBits(u32);

impl TableEntryBits {
    fn kind(&self) -> FieldKind {
        unsafe { std::mem::transmute(self.0 as u8) }
    }

    fn has_bit_idx(&self) -> u32 {
        (self.0 >> 8) & 0xFF
    }

    fn offset(&self) -> u32 {
        self.0 >> 16
    }
}

impl Table {
    fn entry(&self, field_number: u32) -> Option<TableEntryBits> {
        if std::hint::unlikely(field_number >= self.num_entries) {
            return None;
        }
        let entry_bits = unsafe {
            let entries_ptr = (self as *const Table).add(1) as *const u32;
            *entries_ptr.add(field_number as usize)
        };
        Some(TableEntryBits(entry_bits))
    }

    fn aux_entry(&self, offset: u32) -> &AuxTableEntry {
        unsafe {
            let aux_table_ptr =
                (self as *const Table as *const u8).add(offset as usize) as *const AuxTableEntry;
            &*aux_table_ptr
        }
    }
}

#[repr(C)]
pub struct TableWithEntries<const NUM_ENTRIES: usize, const NUM_AUX_ENTRIES: usize>(
    pub Table,
    pub [TableEntry; NUM_ENTRIES],
    pub [AuxTableEntry; NUM_AUX_ENTRIES],
);

struct StackEntry {
    obj: *mut Object,
    table: *const Table,
    delta_limit_or_group_tag: isize,
}

impl StackEntry {
    fn into_context<'a>(
        self,
        mut limit: isize,
        field_number: Option<u32>,
    ) -> Option<ParseContext<'a>> {
        if let Some(field_number) = field_number {
            if -self.delta_limit_or_group_tag != field_number as isize {
                return None;
            }
        } else {
            if self.delta_limit_or_group_tag < 0 {
                return None;
            }
            limit += self.delta_limit_or_group_tag;
        }
        Some(ParseContext {
            limit,
            obj: unsafe { &mut *self.obj },
            table: unsafe { &*self.table },
        })
    }
}

#[repr(C)]
struct ParseContext<'a> {
    limit: isize, // relative to end
    obj: &'a mut Object,
    table: &'a Table,
}

impl<'a> ParseContext<'a> {
    fn limited_end(&self, end: NonNull<u8>) -> NonNull<u8> {
        unsafe { end.offset(self.limit.min(0)) }
    }

    fn push_limit(
        &mut self,
        len: isize,
        cursor: ReadCursor,
        end: NonNull<u8>,
        stack: &mut Stack<StackEntry>,
    ) -> Option<NonNull<u8>> {
        let new_limit = cursor - end + len;
        let delta_limit = self.limit - new_limit;
        if delta_limit < 0 {
            return None;
        }
        stack.push(StackEntry {
            obj: self.obj,
            table: self.table,
            delta_limit_or_group_tag: delta_limit,
        })?;
        self.limit = new_limit;
        Some(self.limited_end(end))
    }

    fn pop_limit(
        &mut self,
        end: NonNull<u8>,
        stack: &mut Stack<StackEntry>,
    ) -> Option<NonNull<u8>> {
        *self = stack.pop()?.into_context(self.limit, None)?;
        Some(self.limited_end(end))
    }

    fn push_group(&mut self, field_number: u32, stack: &mut Stack<StackEntry>) -> Option<()> {
        stack.push(StackEntry {
            obj: self.obj,
            table: self.table,
            delta_limit_or_group_tag: -(field_number as isize),
        })?;
        Some(())
    }

    fn pop_group(&mut self, field_number: u32, stack: &mut Stack<StackEntry>) -> Option<()> {
        *self = stack.pop()?.into_context(self.limit, Some(field_number))?;
        Some(())
    }

    fn set<T>(&mut self, entry: TableEntryBits, val: T) {
        self.obj.set(entry.offset(), entry.has_bit_idx(), val);
    }

    fn add<T>(&mut self, entry: TableEntryBits, val: T) {
        self.obj.add(entry.offset(), val);
    }
}

impl Object {
    fn get_or_create_child_object<'a>(
        &'a mut self,
        aux_entry: &AuxTableEntry,
    ) -> (&'a mut Object, &'a Table) {
        let field = self.ref_mut::<*mut Object>(aux_entry.offset);
        let child_table = unsafe { &*aux_entry.child_table };
        let child = if (*field).is_null() {
            let child = Self::create(child_table.size);
            *field = child;
            child
        } else {
            unsafe { &mut **field }
        };
        (child, child_table)
    }

    fn add_child_object<'a>(
        &'a mut self,
        aux_entry: &AuxTableEntry,
    ) -> (&'a mut Object, &'a Table) {
        let field = self.ref_mut::<RepeatedField<*mut Object>>(aux_entry.offset);
        let child_table = unsafe { &*aux_entry.child_table };
        let child = Self::create(child_table.size);
        field.push(child);
        (child, child_table)
    }
}

#[must_use]
fn validate_wire_type(tag: u32, expected_wire_type: u8) -> bool {
    (tag & 7) == expected_wire_type as u32
}

type ParseLoopResult<'a> = Option<(ReadCursor, isize, ParseObject<'a>)>;

fn parse_string<'a>(
    limit: isize,
    bytes: &'a mut Bytes,
    mut cursor: ReadCursor,
    end: NonNull<u8>,
    stack: &mut Stack<StackEntry>,
) -> ParseLoopResult<'a> {
    if limit > SLOP_SIZE as isize {
        bytes.append(cursor.read_slice(SLOP_SIZE as isize - (cursor - end)));
        return Some((cursor, limit, ParseObject::Bytes(bytes)));
    }
    bytes.append(cursor.read_slice(limit - (cursor - end)));
    let ctx = stack.pop()?.into_context(limit, None)?;
    parse_loop(ctx, cursor, end, stack)
}

#[inline(never)]
fn parse_loop<'a>(
    mut ctx: ParseContext<'a>,
    mut cursor: ReadCursor,
    end: NonNull<u8>,
    stack: &mut Stack<StackEntry>,
) -> ParseLoopResult<'a> {
    let mut limited_end = ctx.limited_end(end);
    // loop popping the stack as needed
    loop {
        // inner parse loop
        'parse_loop: while cursor < limited_end {
            let tag = cursor.read_tag()?;
            // println!("tag: {:o}", tag);
            let field_number = tag >> 3;
            if let Some(entry) = ctx.table.entry(field_number) {
                'unknown: {
                    match entry.kind() {
                        FieldKind::Unknown => {
                            if std::hint::unlikely(tag == 0) {
                                if stack.is_empty() {
                                    return Some((cursor, ctx.limit, ParseObject::None));
                                }
                                return None;
                            }
                            return None;
                        }
                        FieldKind::Varint64 => {
                            // varint64
                            if !validate_wire_type(tag, 0) {
                                break 'unknown;
                            }
                            let value = cursor.read_varint()?;
                            ctx.set(entry, value);
                        }
                        FieldKind::Varint32 => {
                            // varint32
                            if !validate_wire_type(tag, 0) {
                                break 'unknown;
                            }
                            let value = cursor.read_varint()?;
                            ctx.set(entry, value as u32);
                        }
                        FieldKind::Varint64Zigzag => {
                            // varint64 zigzag
                            if !validate_wire_type(tag, 0) {
                                break 'unknown;
                            }
                            let value = cursor.read_varint()?;
                            ctx.set(entry, zigzag_decode(value));
                        }
                        FieldKind::Varint32Zigzag => {
                            // varint32 zigzag
                            if !validate_wire_type(tag, 0) {
                                break 'unknown;
                            }
                            let value = cursor.read_varint()?;
                            ctx.set(entry, zigzag_decode(value) as u32);
                        }
                        FieldKind::Fixed64 => {
                            // fixed64
                            if !validate_wire_type(tag, 1) {
                                break 'unknown;
                            }
                            let value = cursor.read_unaligned::<u64>();
                            ctx.set(entry, value);
                        }
                        FieldKind::Fixed32 => {
                            // fixed32
                            if !validate_wire_type(tag, 5) {
                                break 'unknown;
                            }
                            let value = cursor.read_unaligned::<u32>();
                            ctx.set(entry, value);
                        }
                        FieldKind::Bytes => {
                            // bytes
                            if !validate_wire_type(tag, 2) {
                                break 'unknown;
                            }
                            let len = cursor.read_size()?;
                            if cursor - limited_end + len <= SLOP_SIZE as isize {
                                ctx.obj.set_bytes(
                                    entry.offset(),
                                    entry.has_bit_idx(),
                                    cursor.read_slice(len),
                                );
                            } else {
                                ctx.push_limit(len, cursor, end, stack)?;
                                let bytes = ctx.obj.set_bytes(
                                    entry.offset(),
                                    entry.has_bit_idx(),
                                    cursor.read_slice(SLOP_SIZE as isize - (cursor - end)),
                                );
                                return Some((cursor, ctx.limit, ParseObject::Bytes(bytes)));
                            }
                        }
                        FieldKind::Message => {
                            // message
                            if !validate_wire_type(tag, 2) {
                                break 'unknown;
                            }
                            let len = cursor.read_size()?;
                            // let end = *std::hint::black_box(&end);
                            let aux_entry = ctx.table.aux_entry(entry.offset());
                            limited_end = ctx.push_limit(len, cursor, end, stack)?;
                            (ctx.obj, ctx.table) = ctx.obj.get_or_create_child_object(aux_entry);
                        }
                        FieldKind::Group => {
                            // start group
                            if !validate_wire_type(tag, 3) {
                                break 'unknown;
                            }
                            let aux_entry = ctx.table.aux_entry(entry.offset());
                            ctx.push_group(field_number, stack)?;
                            (ctx.obj, ctx.table) = ctx.obj.get_or_create_child_object(aux_entry);
                        }
                        FieldKind::RepeatedVarint64 => {
                            // varint64
                            if !validate_wire_type(tag, 0) {
                                break 'unknown;
                            }
                            let value = cursor.read_varint()?;
                            ctx.add(entry, value);
                        }
                        FieldKind::RepeatedVarint32 => {
                            // varint32
                            if !validate_wire_type(tag, 0) {
                                break 'unknown;
                            }
                            let value = cursor.read_varint()?;
                            ctx.add(entry, value as u32);
                        }
                        FieldKind::RepeatedVarint64Zigzag => {
                            // varint64 zigzag
                            if !validate_wire_type(tag, 0) {
                                break 'unknown;
                            }
                            let value = cursor.read_varint()?;
                            ctx.add(entry, zigzag_decode(value));
                        }
                        FieldKind::RepeatedVarint32Zigzag => {
                            // varint32 zigzag
                            if !validate_wire_type(tag, 0) {
                                break 'unknown;
                            }
                            let value = cursor.read_varint()?;
                            ctx.add(entry, zigzag_decode(value) as u32);
                        }
                        FieldKind::RepeatedFixed64 => {
                            // fixed64
                            if !validate_wire_type(tag, 1) {
                                break 'unknown;
                            }
                            let value = cursor.read_unaligned::<u64>();
                            ctx.add(entry, value);
                        }
                        FieldKind::RepeatedFixed32 => {
                            // fixed32
                            if !validate_wire_type(tag, 5) {
                                break 'unknown;
                            }
                            let value = cursor.read_unaligned::<u32>();
                            ctx.add(entry, value);
                        }
                        FieldKind::RepeatedBytes => {
                            // bytes
                            if !validate_wire_type(tag, 2) {
                                break 'unknown;
                            }
                            let len = cursor.read_size()?;
                            if cursor - limited_end + len <= SLOP_SIZE as isize {
                                ctx.obj.add_bytes(entry.offset(), cursor.read_slice(len));
                            } else {
                                ctx.push_limit(len, cursor, end, stack)?;
                                let bytes = ctx.obj.add_bytes(
                                    entry.offset(),
                                    cursor.read_slice(SLOP_SIZE as isize - (cursor - end)),
                                );
                                return Some((cursor, ctx.limit, ParseObject::Bytes(bytes)));
                            }
                        }
                        FieldKind::RepeatedMessage => {
                            // message
                            if !validate_wire_type(tag, 2) {
                                break 'unknown;
                            }
                            let len = cursor.read_size()?;
                            let aux_entry = ctx.table.aux_entry(entry.offset());
                            limited_end = ctx.push_limit(len, cursor, end, stack)?;
                            (ctx.obj, ctx.table) = ctx.obj.add_child_object(aux_entry);
                        }
                        FieldKind::RepeatedGroup => {
                            // start group
                            if !validate_wire_type(tag, 3) {
                                break 'unknown;
                            }
                            let aux_entry = ctx.table.aux_entry(entry.offset());
                            ctx.push_group(field_number, stack)?;
                            (ctx.obj, ctx.table) = ctx.obj.add_child_object(aux_entry);
                        }
                    }
                    continue 'parse_loop;
                }
            }
            // unknown field
            if std::hint::unlikely((tag & 7) == 4) {
                ctx.pop_group(field_number, stack)?;
                continue;
            }
            return None;
        }
        if cursor - end == ctx.limit {
            if stack.is_empty() {
                return Some((cursor, ctx.limit, ParseObject::None));
            }
            limited_end = ctx.pop_limit(end, stack)?;
            continue;
        }
        if cursor >= end {
            break;
        }
        if cursor != limited_end {
            return None;
        }
    }
    Some((cursor, ctx.limit, ParseObject::Message(ctx.obj, ctx.table)))
}

enum ParseObject<'a> {
    None,
    Message(&'a mut Object, &'a Table),
    Bytes(&'a mut Bytes),
}

struct ResumeableState<'a> {
    limit: isize,
    object: ParseObject<'a>,
    overrun: isize,
}

impl<'a> ResumeableState<'a> {
    fn go_parse(mut self, buf: &[u8], stack: &mut Stack<StackEntry>) -> Option<Self> {
        let len = buf.len() as isize;
        self.limit -= len;
        if self.overrun >= len {
            self.overrun -= len;
            return Some(self);
        }
        let (mut cursor, end) = ReadCursor::new(buf);
        cursor += self.overrun;
        let (new_cursor, new_limit, new_object) = match self.object {
            ParseObject::Message(obj, table) => {
                let ctx = ParseContext {
                    limit: self.limit,
                    obj,
                    table,
                };
                parse_loop(ctx, cursor, end, stack)?
            }
            ParseObject::Bytes(bytes) => parse_string(self.limit, bytes, cursor, end, stack)?,
            ParseObject::None => unreachable!(),
        };
        self.limit = new_limit;
        self.object = new_object;
        self.overrun = new_cursor - end;
        Some(self)
    }
}

#[repr(C)]
pub struct ResumeableParse<'a, const STACK_DEPTH: usize> {
    state: MaybeUninit<ResumeableState<'a>>,
    patch_buffer: [u8; SLOP_SIZE * 2],
    stack: StackWithStorage<StackEntry, STACK_DEPTH>,
}

impl<'a, const STACK_DEPTH: usize> ResumeableParse<'a, STACK_DEPTH> {
    pub fn new<T: Protobuf + ?Sized>(obj: &'a mut T, limit: isize) -> Self {
        let object = ParseObject::Message(obj.as_object_mut(), T::decoding_table());
        Self {
            state: MaybeUninit::new(ResumeableState {
                limit,
                object,
                overrun: SLOP_SIZE as isize,
            }),
            patch_buffer: [0; SLOP_SIZE * 2],
            stack: Default::default(),
        }
    }

    #[must_use]
    pub fn resume(&mut self, buf: &[u8]) -> bool {
        self.resume_impl(buf).is_some()
    }

    #[must_use]
    pub fn finish(self) -> bool {
        let ResumeableParse {
            state,
            patch_buffer,
            mut stack,
        } = self;
        let state = unsafe { state.assume_init() };
        let Some(state) = state.go_parse(&patch_buffer[..SLOP_SIZE], &mut stack) else {
            return false;
        };
        state.overrun == 0 && matches!(state.object, ParseObject::Message(_, _)) && stack.is_empty()
    }

    fn resume_impl(&mut self, buf: &[u8]) -> Option<()> {
        let size = buf.len();
        let mut state = unsafe { self.state.assume_init_read() };
        if buf.len() > SLOP_SIZE {
            self.patch_buffer[SLOP_SIZE..].copy_from_slice(&buf[..SLOP_SIZE]);
            state = state.go_parse(&self.patch_buffer[..SLOP_SIZE], &mut self.stack)?;
            state = state.go_parse(&buf[..size - SLOP_SIZE], &mut self.stack)?;
            self.patch_buffer[..SLOP_SIZE].copy_from_slice(&buf[size - SLOP_SIZE..]);
        } else {
            self.patch_buffer[SLOP_SIZE..SLOP_SIZE + size].copy_from_slice(buf);
            state = state.go_parse(&self.patch_buffer[..size], &mut self.stack)?;
            self.patch_buffer.copy_within(size..size + SLOP_SIZE, 0);
        }
        self.state.write(state);
        Some(())
    }
}
