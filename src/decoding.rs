use core::mem::MaybeUninit;
use core::ptr::NonNull;

use crate::Protobuf;
use crate::base::Object;
use crate::containers::{Bytes, RepeatedField};
use crate::tables::{AuxTableEntry, Table};
use crate::utils::{Stack, StackWithStorage};
use crate::wire::{FieldKind, ReadCursor, SLOP_SIZE, zigzag_decode};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TableEntry(pub u32);

impl TableEntry {
    pub const fn new(kind: FieldKind, has_bit_idx: u32, offset: usize) -> Self {
        TableEntry(((offset & 0xFFFF) as u32) << 16 | has_bit_idx << 8 | (kind as u8 as u32))
    }

    pub(crate) fn kind(&self) -> FieldKind {
        unsafe { std::mem::transmute(self.0 as u8) }
    }

    pub(crate) fn has_bit_idx(&self) -> u32 {
        (self.0 >> 8) & 0xFF
    }

    pub(crate) fn offset(&self) -> u32 {
        self.0 >> 16
    }

    pub(crate) fn aux_offset(&self) -> u32 {
        self.0 >> 16
    }
}

impl Table {
    #[inline(always)]
    pub(crate) fn entry(&self, field_number: u32) -> Option<TableEntry> {
        let entries = self.decode_entries();
        if field_number >= entries.len() as u32 {
            return None;
        }
        Some(entries[field_number as usize])
    }

    #[inline(always)]
    pub(crate) fn aux_entry_decode(&self, entry: TableEntry) -> AuxTableEntry {
        let offset = entry.aux_offset();
        self.aux_entry(offset as usize)
    }
}

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
    ) -> Option<DecodeObjectState<'a>> {
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
        Some(DecodeObjectState {
            limit,
            obj: unsafe { &mut *self.obj },
            table: unsafe { &*self.table },
        })
    }
}

enum DecodeObject<'a> {
    None,
    Message(&'a mut Object, &'a Table),
    Bytes(&'a mut Bytes),
    SkipLengthDelimited,
    SkipGroup,
}

#[repr(C)]
struct DecodeObjectState<'a> {
    limit: isize, // relative to end
    obj: &'a mut Object,
    table: &'a Table,
}

impl<'a> DecodeObjectState<'a> {
    fn limited_end(&self, end: NonNull<u8>) -> NonNull<u8> {
        unsafe { end.offset(self.limit.min(0)) }
    }

    #[inline(always)]
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

    #[inline(always)]
    fn pop_limit(
        &mut self,
        end: NonNull<u8>,
        stack: &mut Stack<StackEntry>,
    ) -> Option<NonNull<u8>> {
        *self = stack.pop()?.into_context(self.limit, None)?;
        Some(self.limited_end(end))
    }

    #[inline(always)]
    fn push_group(&mut self, field_number: u32, stack: &mut Stack<StackEntry>) -> Option<()> {
        stack.push(StackEntry {
            obj: self.obj,
            table: self.table,
            delta_limit_or_group_tag: -(field_number as isize),
        })?;
        Some(())
    }

    #[inline(always)]
    fn pop_group(&mut self, field_number: u32, stack: &mut Stack<StackEntry>) -> Option<()> {
        *self = stack.pop()?.into_context(self.limit, Some(field_number))?;
        Some(())
    }

    #[inline(always)]
    fn set<T>(&mut self, entry: TableEntry, val: T) {
        self.obj.set(entry.offset(), entry.has_bit_idx(), val);
    }

    #[inline(always)]
    fn add<T>(&mut self, entry: TableEntry, val: T, arena: &mut crate::arena::Arena) {
        self.obj.add(entry.aux_offset(), val, arena);
    }

    #[inline(always)]
    fn set_bytes(
        &mut self,
        entry: TableEntry,
        slice: &[u8],
        arena: &mut crate::arena::Arena,
    ) -> &'a mut Bytes {
        unsafe {
            core::mem::transmute(self.obj.set_bytes(
                entry.offset(),
                entry.has_bit_idx(),
                slice,
                arena,
            ))
        }
    }

    #[inline(always)]
    fn add_bytes(
        &mut self,
        entry: TableEntry,
        slice: &[u8],
        arena: &mut crate::arena::Arena,
    ) -> &'a mut Bytes {
        unsafe { core::mem::transmute(self.obj.add_bytes(entry.aux_offset(), slice, arena)) }
    }

    #[inline(always)]
    fn get_or_create_child_object(
        &mut self,
        entry: TableEntry,
        arena: &mut crate::arena::Arena,
    ) -> (&'a mut Object, &'a Table) {
        let aux_entry = self.table.aux_entry_decode(entry);
        let field = self.obj.ref_mut::<*mut Object>(aux_entry.offset);
        let child_table = unsafe { &*aux_entry.child_table };
        let child = if (*field).is_null() {
            let child = Object::create(child_table.size as u32, arena);
            *field = child;
            child
        } else {
            unsafe { &mut **field }
        };
        (child, child_table)
    }

    #[inline(always)]
    fn add_child_object(
        &mut self,
        entry: TableEntry,
        arena: &mut crate::arena::Arena,
    ) -> (&'a mut Object, &'a Table) {
        let aux_entry = self.table.aux_entry_decode(entry);
        let field = self
            .obj
            .ref_mut::<RepeatedField<*mut Object>>(aux_entry.offset);
        let child_table = unsafe { &*aux_entry.child_table };
        let child = Object::create(child_table.size as u32, arena);
        field.push(child, arena);
        (child, child_table)
    }
}

type DecodeLoopResult<'a> = Option<(ReadCursor, isize, DecodeObject<'a>)>;

#[inline(never)]
fn skip_length_delimited<'a>(
    limit: isize,
    mut cursor: ReadCursor,
    end: NonNull<u8>,
    stack: &mut Stack<StackEntry>,
    arena: &mut crate::arena::Arena,
) -> DecodeLoopResult<'a> {
    if limit > SLOP_SIZE as isize {
        cursor.read_slice(SLOP_SIZE as isize - (cursor - end));
        return Some((cursor, limit, DecodeObject::SkipLengthDelimited));
    }
    cursor.read_slice(limit - (cursor - end));
    let ctx = stack.pop()?.into_context(limit, None)?;
    decode_loop(ctx, cursor, end, stack, arena)
}

#[inline(never)]
fn skip_group<'a>(
    limit: isize,
    mut cursor: ReadCursor,
    end: NonNull<u8>,
    stack: &mut Stack<StackEntry>,
    arena: &mut crate::arena::Arena,
) -> DecodeLoopResult<'a> {
    let mut limited_end = unsafe { end.offset(limit.min(0)) };
    // loop popping the stack as needed
    loop {
        // inner parse loop
        while cursor < limited_end {
            let tag = cursor.read_tag()?;
            let wire_type = tag & 7;
            let field_number = tag >> 3;
            match wire_type {
                0 => {
                    // varint
                    let _ = cursor.read_varint()?;
                }
                1 => {
                    // fixed64
                    cursor += 8;
                }
                2 => {
                    // length-delimited
                    let len = cursor.read_size()?;
                    if cursor - limited_end + len <= SLOP_SIZE as isize {
                        cursor.read_slice(len);
                    } else {
                        let new_limit = cursor - end + len;
                        let delta_limit = limit - new_limit;
                        stack.push(StackEntry {
                            obj: core::ptr::null_mut(),
                            table: core::ptr::null(),
                            delta_limit_or_group_tag: delta_limit,
                        });
                        return Some((cursor, new_limit, DecodeObject::SkipLengthDelimited));
                    }
                }
                3 => {
                    // start group
                    stack.push(StackEntry {
                        obj: core::ptr::null_mut(),
                        table: core::ptr::null(),
                        delta_limit_or_group_tag: -(field_number as isize),
                    })?;
                }
                4 => {
                    // end group
                    let Some(StackEntry {
                        obj,
                        table,
                        delta_limit_or_group_tag,
                    }) = stack.pop()
                    else {
                        return None;
                    };
                    if -delta_limit_or_group_tag != field_number as isize {
                        return None;
                    }
                    if !obj.is_null() {
                        let ctx = DecodeObjectState {
                            limit,
                            obj: unsafe { &mut *obj },
                            table: unsafe { &*table },
                        };
                        return decode_loop(ctx, cursor, end, stack, arena);
                    }
                }
                5 => {
                    // fixed32
                    cursor += 4;
                }
                _ => {
                    return None;
                }
            }
        }
        if cursor - end == limit {
            if stack.is_empty() {
                return Some((cursor, limit, DecodeObject::None));
            }
            limited_end = stack.pop()?.into_context(limit, None)?.limited_end(end);
            continue;
        }
        if cursor >= end {
            break;
        }
        if cursor != limited_end {
            return None;
        }
    }
    Some((cursor, limit, DecodeObject::SkipGroup))
}

#[inline(never)]
fn decode_string<'a>(
    limit: isize,
    bytes: &'a mut Bytes,
    mut cursor: ReadCursor,
    end: NonNull<u8>,
    stack: &mut Stack<StackEntry>,
    arena: &mut crate::arena::Arena,
) -> DecodeLoopResult<'a> {
    if limit > SLOP_SIZE as isize {
        bytes.append(
            cursor.read_slice(SLOP_SIZE as isize - (cursor - end)),
            arena,
        );
        return Some((cursor, limit, DecodeObject::Bytes(bytes)));
    }
    bytes.append(cursor.read_slice(limit - (cursor - end)), arena);
    let ctx = stack.pop()?.into_context(limit, None)?;
    decode_loop(ctx, cursor, end, stack, arena)
}

#[inline(never)]
fn decode_loop<'a>(
    mut ctx: DecodeObjectState<'a>,
    mut cursor: ReadCursor,
    end: NonNull<u8>,
    stack: &mut Stack<StackEntry>,
    arena: &mut crate::arena::Arena,
) -> DecodeLoopResult<'a> {
    let mut limited_end = ctx.limited_end(end);
    // loop popping the stack as needed
    loop {
        // inner parse loop
        'parse_loop: while cursor < limited_end {
            let tag = cursor.read_tag()?;
            let field_number = tag >> 3;
            if false {
                let descriptor = ctx.table.descriptor;
                let field = descriptor
                    .field()
                    .iter()
                    .find(|f| f.number() as u32 == field_number);
                if field.is_none() {
                    // field not found in descriptor, treat as unknown
                    println!(
                        "Msg {} Unknown Field number: {}",
                        descriptor.name(),
                        field_number
                    );
                } else {
                    let field = field.unwrap();
                    println!(
                        "Msg {} Field number: {}, Field name {}",
                        descriptor.name(),
                        field_number,
                        field.name()
                    );
                }
            }
            if let Some(entry) = ctx.table.entry(field_number) {
                'unknown: {
                    match entry.kind() {
                        FieldKind::Varint64 => {
                            if tag & 7 != 0 {
                                break 'unknown;
                            };
                            ctx.set(entry, cursor.read_varint()?);
                        }
                        FieldKind::Varint32 => {
                            if tag & 7 != 0 {
                                break 'unknown;
                            };
                            ctx.set(entry, cursor.read_varint()? as u32);
                        }
                        FieldKind::Varint64Zigzag => {
                            if tag & 7 != 0 {
                                break 'unknown;
                            };
                            ctx.set(entry, zigzag_decode(cursor.read_varint()?));
                        }
                        FieldKind::Varint32Zigzag => {
                            if tag & 7 != 0 {
                                break 'unknown;
                            };
                            ctx.set(entry, zigzag_decode(cursor.read_varint()?) as i32);
                        }
                        FieldKind::Bool => {
                            if tag & 7 != 0 {
                                break 'unknown;
                            };
                            let val = cursor.read_varint()?;
                            ctx.set(entry, val != 0);
                        }
                        FieldKind::Fixed64 => {
                            if tag & 7 != 1 {
                                break 'unknown;
                            };
                            ctx.set(entry, cursor.read_unaligned::<u64>());
                        }
                        FieldKind::Fixed32 => {
                            if tag & 7 != 5 {
                                break 'unknown;
                            };
                            ctx.set(entry, cursor.read_unaligned::<u32>());
                        }
                        FieldKind::Bytes => {
                            if tag & 7 != 2 {
                                break 'unknown;
                            };
                            let len = cursor.read_size()?;
                            if cursor - limited_end + len <= SLOP_SIZE as isize {
                                ctx.set_bytes(entry, cursor.read_slice(len), arena);
                            } else {
                                ctx.push_limit(len, cursor, end, stack)?;
                                let bytes = ctx.set_bytes(
                                    entry,
                                    cursor.read_slice(SLOP_SIZE as isize - (cursor - end)),
                                    arena,
                                );
                                return Some((cursor, ctx.limit, DecodeObject::Bytes(bytes)));
                            }
                        }
                        FieldKind::Message => {
                            if tag & 7 != 2 {
                                break 'unknown;
                            };
                            let len = cursor.read_size()?;
                            limited_end = ctx.push_limit(len, cursor, end, stack)?;
                            (ctx.obj, ctx.table) = ctx.get_or_create_child_object(entry, arena);
                        }
                        FieldKind::Group => {
                            if tag & 7 != 3 {
                                break 'unknown;
                            };
                            ctx.push_group(field_number, stack)?;
                            (ctx.obj, ctx.table) = ctx.get_or_create_child_object(entry, arena);
                        }
                        FieldKind::RepeatedVarint64 => {
                            if tag & 7 != 0 {
                                break 'unknown;
                            };
                            ctx.add(entry, cursor.read_varint()?, arena);
                        }
                        FieldKind::RepeatedVarint32 => {
                            if tag & 7 != 0 {
                                break 'unknown;
                            };
                            ctx.add(entry, cursor.read_varint()? as u32, arena);
                        }
                        FieldKind::RepeatedVarint64Zigzag => {
                            if tag & 7 != 0 {
                                break 'unknown;
                            };
                            ctx.add(entry, zigzag_decode(cursor.read_varint()?), arena);
                        }
                        FieldKind::RepeatedVarint32Zigzag => {
                            if tag & 7 != 0 {
                                break 'unknown;
                            };
                            ctx.add(entry, zigzag_decode(cursor.read_varint()?) as i32, arena);
                        }
                        FieldKind::RepeatedBool => {
                            if tag & 7 != 0 {
                                break 'unknown;
                            };
                            let val = cursor.read_varint()?;
                            ctx.add(entry, val != 0, arena);
                        }
                        FieldKind::RepeatedFixed64 => {
                            if tag & 7 != 1 {
                                break 'unknown;
                            };
                            ctx.add(entry, cursor.read_unaligned::<u64>(), arena);
                        }
                        FieldKind::RepeatedFixed32 => {
                            if tag & 7 != 5 {
                                break 'unknown;
                            };
                            ctx.add(entry, cursor.read_unaligned::<u32>(), arena);
                        }
                        FieldKind::RepeatedBytes => {
                            if tag & 7 != 2 {
                                break 'unknown;
                            };
                            let len = cursor.read_size()?;
                            if cursor - limited_end + len <= SLOP_SIZE as isize {
                                ctx.add_bytes(entry, cursor.read_slice(len), arena);
                            } else {
                                ctx.push_limit(len, cursor, end, stack)?;
                                let bytes = ctx.add_bytes(
                                    entry,
                                    cursor.read_slice(SLOP_SIZE as isize - (cursor - end)),
                                    arena,
                                );
                                return Some((cursor, ctx.limit, DecodeObject::Bytes(bytes)));
                            }
                        }
                        FieldKind::RepeatedMessage => {
                            if tag & 7 != 2 {
                                break 'unknown;
                            };
                            let len = cursor.read_size()?;
                            limited_end = ctx.push_limit(len, cursor, end, stack)?;
                            (ctx.obj, ctx.table) = ctx.add_child_object(entry, arena);
                        }
                        FieldKind::RepeatedGroup => {
                            if tag & 7 != 3 {
                                break 'unknown;
                            };
                            ctx.push_group(field_number, stack)?;
                            (ctx.obj, ctx.table) = ctx.add_child_object(entry, arena);
                        }
                        FieldKind::Unknown => {
                            break 'unknown;
                        }
                    }
                    continue 'parse_loop;
                }
            };
            // unknown field
            if field_number == 0 {
                if tag == 0 {
                    // 0 byte can used to terminate parsing, but only if stack is empty
                    if stack.is_empty() {
                        return Some((cursor, ctx.limit, DecodeObject::None));
                    }
                    return None;
                }
                // field number 0 is invalid
                return None;
            }
            match tag & 7 {
                0 => {
                    // varint
                    let _ = cursor.read_varint()?;
                }
                1 => {
                    // fixed64
                    cursor += 8;
                }
                2 => {
                    // length-delimited
                    let len = cursor.read_size()?;
                    if cursor - limited_end + len <= SLOP_SIZE as isize {
                        cursor.read_slice(len);
                    } else {
                        ctx.push_limit(len, cursor, end, stack)?;
                        return Some((cursor, ctx.limit, DecodeObject::SkipLengthDelimited));
                    }
                }
                3 => {
                    // start group
                    // push to stack until end group
                    ctx.push_group(field_number, stack)?;
                    return skip_group(ctx.limit, cursor, end, stack, arena);
                }
                4 => {
                    // end group
                    ctx.pop_group(field_number, stack)?;
                }
                5 => {
                    // fixed32
                    cursor += 4;
                }
                _ => {
                    return None;
                }
            }
        }
        if cursor - end == ctx.limit {
            if stack.is_empty() {
                return Some((cursor, ctx.limit, DecodeObject::None));
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
    Some((cursor, ctx.limit, DecodeObject::Message(ctx.obj, ctx.table)))
}

struct ResumeableState<'a> {
    limit: isize,
    object: DecodeObject<'a>,
    overrun: isize,
}

impl<'a> ResumeableState<'a> {
    fn go_decode(
        mut self,
        buf: &[u8],
        stack: &mut Stack<StackEntry>,
        arena: &mut crate::arena::Arena,
    ) -> Option<Self> {
        let len = buf.len() as isize;
        self.limit -= len;
        if self.overrun >= len {
            self.overrun -= len;
            return Some(self);
        }
        let (mut cursor, end) = ReadCursor::new(buf);
        cursor += self.overrun;
        let (new_cursor, new_limit, new_object) = match self.object {
            DecodeObject::Message(obj, table) => {
                let ctx = DecodeObjectState {
                    limit: self.limit,
                    obj,
                    table,
                };
                decode_loop(ctx, cursor, end, stack, arena)?
            }
            DecodeObject::Bytes(bytes) => {
                decode_string(self.limit, bytes, cursor, end, stack, arena)?
            }
            DecodeObject::SkipLengthDelimited => {
                skip_length_delimited(self.limit, cursor, end, stack, arena)?
            }
            DecodeObject::SkipGroup => skip_group(self.limit, cursor, end, stack, arena)?,
            DecodeObject::None => unreachable!(),
        };
        self.limit = new_limit;
        self.object = new_object;
        self.overrun = new_cursor - end;
        Some(self)
    }
}

#[repr(C)]
pub struct ResumeableDecode<'a, const STACK_DEPTH: usize> {
    state: MaybeUninit<ResumeableState<'a>>,
    patch_buffer: [u8; SLOP_SIZE * 2],
    stack: StackWithStorage<StackEntry, STACK_DEPTH>,
}

impl<'a, const STACK_DEPTH: usize> ResumeableDecode<'a, STACK_DEPTH> {
    pub fn new<T: Protobuf + ?Sized>(obj: &'a mut T, limit: isize) -> Self {
        let object = DecodeObject::Message(obj.as_object_mut(), T::table());
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
    pub fn resume(&mut self, buf: &[u8], arena: &mut crate::arena::Arena) -> bool {
        self.resume_impl(buf, arena).is_some()
    }

    #[must_use]
    pub fn finish(self, arena: &mut crate::arena::Arena) -> bool {
        let ResumeableDecode {
            state,
            patch_buffer,
            mut stack,
        } = self;
        let state = unsafe { state.assume_init() };
        let Some(state) = state.go_decode(&patch_buffer[..SLOP_SIZE], &mut stack, arena) else {
            return false;
        };
        state.overrun == 0
            && matches!(state.object, DecodeObject::Message(_, _))
            && stack.is_empty()
    }

    fn resume_impl(&mut self, buf: &[u8], arena: &mut crate::arena::Arena) -> Option<()> {
        let size = buf.len();
        let mut state = unsafe { self.state.assume_init_read() };
        if buf.len() > SLOP_SIZE {
            self.patch_buffer[SLOP_SIZE..].copy_from_slice(&buf[..SLOP_SIZE]);
            state = state.go_decode(&self.patch_buffer[..SLOP_SIZE], &mut self.stack, arena)?;
            state = state.go_decode(&buf[..size - SLOP_SIZE], &mut self.stack, arena)?;
            self.patch_buffer[..SLOP_SIZE].copy_from_slice(&buf[size - SLOP_SIZE..]);
        } else {
            self.patch_buffer[SLOP_SIZE..SLOP_SIZE + size].copy_from_slice(buf);
            state = state.go_decode(&self.patch_buffer[..size], &mut self.stack, arena)?;
            self.patch_buffer.copy_within(size..size + SLOP_SIZE, 0);
        }
        self.state.write(state);
        Some(())
    }
}
