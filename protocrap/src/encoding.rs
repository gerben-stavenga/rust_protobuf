use core::{mem::MaybeUninit, ptr::NonNull};

use crate::{
    Protobuf,
    base::Object,
    containers::Bytes,
    utils::{Stack, StackWithStorage},
    wire::{FieldKind, SLOP_SIZE, WriteCursor, zigzag_encode},
};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TableEntry {
    pub has_bit: u8,
    pub kind: FieldKind,
    pub offset: u16,
    pub encoded_tag: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AuxTableEntry {
    pub offset: usize,
    pub child_table: *const [TableEntry],
}

unsafe impl Send for AuxTableEntry {}
unsafe impl Sync for AuxTableEntry {}

pub struct TableWithEntries<const N: usize, const M: usize>(
    pub [TableEntry; N],
    pub [AuxTableEntry; M],
);

fn aux_entry<'a>(offset: usize, table: *const [TableEntry]) -> (usize, &'a [TableEntry]) {
    unsafe {
        let thin_ptr = table as *const u8;
        let AuxTableEntry {
            offset,
            child_table: table,
        } = *(thin_ptr.add(offset) as *const AuxTableEntry);
        (offset, &*table)
    }
}

struct StackEntry {
    obj: *const Object,
    table: *const [TableEntry],
    field_idx: usize,
    rep_field_idx: usize,
    byte_count: isize,
    tag: u32,
}

impl StackEntry {
    fn into_context<'a>(self) -> (ObjectEncodeState<'a>, u32, isize) {
        (
            ObjectEncodeState {
                obj: unsafe { &*self.obj },
                table: unsafe { &*self.table },
                field_idx: self.field_idx,
                rep_field_idx: self.rep_field_idx,
            },
            self.tag,
            self.byte_count,
        )
    }
}

enum EncodeObject<'a> {
    Done,
    Object(ObjectEncodeState<'a>),
    String(&'a [u8]),
}

struct ObjectEncodeState<'a> {
    obj: &'a Object,
    table: &'a [TableEntry],
    field_idx: usize,
    rep_field_idx: usize,
}

impl<'a> ObjectEncodeState<'a> {
    fn new(obj: &'a Object, table: &'a [TableEntry]) -> Self {
        Self {
            obj,
            table,
            field_idx: table.len(),
            rep_field_idx: 0,
        }
    }

    fn push(&mut self, tag: u32, byte_count: isize, stack: &mut Stack<StackEntry>) -> Option<()> {
        stack.push(StackEntry {
            obj: self.obj,
            table: self.table,
            field_idx: self.field_idx,
            rep_field_idx: self.rep_field_idx,
            tag,
            byte_count,
        })?;
        Some(())
    }

    fn pop(&mut self, stack: &mut Stack<StackEntry>) -> Option<(u32, isize)> {
        let (ctx, tag, byte_count) = stack.pop()?.into_context();
        *self = ctx;
        Some((tag, byte_count))
    }

    fn has_bit(&self, has_bit_idx: u8) -> bool {
        self.obj.has_bit(has_bit_idx)
    }

    fn get<T>(&self, offset: usize) -> T
    where
        T: Copy,
    {
        self.obj.get::<T>(offset)
    }

    fn get_slice<T>(&self, offset: usize) -> &'a [T] {
        self.obj.get_slice::<T>(offset)
    }

    fn bytes(&self, offset: usize) -> &'a [u8] {
        self.obj.bytes(offset)
    }
}

fn encode_bytes<'a>(
    bytes: &'a [u8],
    mut cursor: WriteCursor,
    begin: NonNull<u8>,
    byte_count: isize,
    stack: &mut Stack<StackEntry>,
) -> EncodeResult<'a> {
    let len = bytes.len();
    assert!(cursor > begin);
    let buffer_size = (cursor - begin) as usize;
    if buffer_size < len {
        cursor.write_slice(&bytes[len - buffer_size..]);
        return Some((cursor, EncodeObject::String(&bytes[..len - buffer_size])));
    }
    cursor.write_slice(bytes);
    let (ctx, tag, old_byte_count) = stack.pop()?.into_context();
    let field_byte_count = count(cursor, begin, byte_count) - old_byte_count;
    cursor.write_varint(field_byte_count as u64);
    cursor.write_tag(tag);
    encode_loop(ctx, cursor, begin, byte_count, stack)
}

// Serialize backwards, so that length prefixes are easy to write.

fn count(cursor: WriteCursor, begin: NonNull<u8>, byte_count: isize) -> isize {
    byte_count - (cursor - begin)
}

type EncodeResult<'a> = Option<(WriteCursor, EncodeObject<'a>)>;

fn write_repeated<T>(
    obj_state: &mut ObjectEncodeState,
    cursor: &mut WriteCursor,
    begin: NonNull<u8>,
    tag: u32,
    slice: &[T],
    write: impl Fn(&mut WriteCursor, &T),
) {
    if obj_state.rep_field_idx == 0 {
        obj_state.rep_field_idx = slice.len();
    }
    while obj_state.rep_field_idx > 0 {
        if *cursor <= begin {
            break;
        }
        obj_state.rep_field_idx -= 1;
        write(cursor, &slice[obj_state.rep_field_idx]);
        cursor.write_tag(tag);
    }
}

fn encode_loop<'a>(
    mut obj_state: ObjectEncodeState<'a>,
    mut cursor: WriteCursor,
    begin: NonNull<u8>,
    byte_count: isize,
    stack: &mut Stack<StackEntry>,
) -> EncodeResult<'a> {
    'out: loop {
        if obj_state.rep_field_idx == 0 {
            while obj_state.field_idx == 0 {
                if stack.is_empty() {
                    return Some((cursor, EncodeObject::Done));
                }
                if cursor <= begin {
                    break 'out;
                }
                let Some((tag, old_byte_count)) = obj_state.pop(stack) else {
                    unreachable!()
                };
                if old_byte_count >= 0 {
                    let field_byte_count = count(cursor, begin, byte_count) - old_byte_count;
                    cursor.write_varint(field_byte_count as u64);
                }
                cursor.write_tag(tag);
                if obj_state.rep_field_idx != 0 {
                    continue 'out;
                }
            }
            assert!(obj_state.rep_field_idx == 0 && obj_state.field_idx > 0);
            obj_state.field_idx -= 1;
        }
        let TableEntry {
            has_bit,
            kind,
            offset,
            encoded_tag: tag,
        } = obj_state.table[obj_state.field_idx];
        let offset = offset as usize;
        match kind {
            FieldKind::Unknown => {
                unreachable!()
            }
            FieldKind::Varint64 => {
                if obj_state.has_bit(has_bit) {
                    if cursor <= begin {
                        break;
                    }
                    cursor.write_varint(obj_state.get::<u64>(offset));
                    cursor.write_tag(tag);
                }
            }
            FieldKind::Varint32 => {
                if obj_state.has_bit(has_bit) {
                    if cursor <= begin {
                        break;
                    }

                    cursor.write_varint(obj_state.get::<u32>(offset) as u64);
                    cursor.write_tag(tag);
                }
            }
            FieldKind::Varint64Zigzag => {
                if obj_state.has_bit(has_bit) {
                    if cursor <= begin {
                        break;
                    }
                    cursor.write_varint(zigzag_encode(obj_state.get::<i64>(offset)));
                    cursor.write_tag(tag);
                }
            }
            FieldKind::Varint32Zigzag => {
                if obj_state.has_bit(has_bit) {
                    if cursor <= begin {
                        break;
                    }
                    cursor.write_varint(zigzag_encode(obj_state.get::<i32>(offset) as i64));
                    cursor.write_tag(tag);
                }
            }
            FieldKind::Fixed64 => {
                if obj_state.has_bit(has_bit) {
                    if cursor <= begin {
                        break;
                    }
                    cursor.write_unaligned(obj_state.get::<u64>(offset));
                    cursor.write_tag(tag);
                }
            }
            FieldKind::Fixed32 => {
                if obj_state.has_bit(has_bit) {
                    if cursor <= begin {
                        break;
                    }
                    cursor.write_unaligned(obj_state.get::<u32>(offset));
                    cursor.write_tag(tag);
                }
            }
            FieldKind::Bytes => {
                if obj_state.has_bit(has_bit) {
                    if cursor <= begin {
                        break;
                    }
                    let bytes = obj_state.bytes(offset);
                    let len = bytes.len();
                    // We don't use slop as we need to write length prefix and tag too.
                    let buffer_size = (cursor - begin) as usize;
                    if buffer_size < len {
                        obj_state.push(tag, count(cursor, begin, byte_count), stack)?;
                        cursor.write_slice(&bytes[len - buffer_size..]);
                        return Some((cursor, EncodeObject::String(&bytes[..len - buffer_size])));
                    }
                    cursor.write_slice(bytes);
                    cursor.write_varint(bytes.len() as u64);
                    cursor.write_tag(tag);
                }
            }
            FieldKind::Message => {
                let (offset, child_table) = aux_entry(offset, obj_state.table);
                let child_ptr = obj_state.get::<*const Object>(offset);
                if !child_ptr.is_null() {
                    obj_state.push(tag, count(cursor, begin, byte_count), stack)?;
                    obj_state = ObjectEncodeState::new(unsafe { &*child_ptr }, child_table);
                }
            }
            FieldKind::Group => {
                let (offset, child_table) = aux_entry(offset, obj_state.table);
                let child_ptr = obj_state.get::<*const Object>(offset);
                if !child_ptr.is_null() {
                    if cursor <= begin {
                        break;
                    }
                    let mut end_tag = tag;
                    end_tag += 1; // Set wire type to END_GROUP
                    cursor.write_tag(end_tag);
                    obj_state.push(tag, -1, stack)?;
                    obj_state = ObjectEncodeState::new(unsafe { &*child_ptr }, child_table);
                }
            }
            FieldKind::RepeatedVarint64 => {
                let slice = obj_state.get_slice::<u64>(offset);
                write_repeated(
                    &mut obj_state,
                    &mut cursor,
                    begin,
                    tag,
                    slice,
                    |cursor, &val| {
                        cursor.write_varint(val);
                    },
                );
            }
            FieldKind::RepeatedVarint32 => {
                let slice = obj_state.get_slice::<u32>(offset);
                write_repeated(
                    &mut obj_state,
                    &mut cursor,
                    begin,
                    tag,
                    slice,
                    |cursor, &val| {
                        cursor.write_varint(val as u64);
                    },
                );
            }
            FieldKind::RepeatedVarint64Zigzag => {
                let slice = obj_state.get_slice::<i64>(offset);
                write_repeated(
                    &mut obj_state,
                    &mut cursor,
                    begin,
                    tag,
                    slice,
                    |cursor, &val| {
                        cursor.write_varint(zigzag_encode(val));
                    },
                );
            }
            FieldKind::RepeatedVarint32Zigzag => {
                let slice = obj_state.get_slice::<i32>(offset);
                write_repeated(
                    &mut obj_state,
                    &mut cursor,
                    begin,
                    tag,
                    slice,
                    |cursor, &val| {
                        cursor.write_varint(zigzag_encode(val as i64));
                    },
                );
            }
            FieldKind::RepeatedFixed64 => {
                let slice = obj_state.get_slice::<u64>(offset);
                write_repeated(
                    &mut obj_state,
                    &mut cursor,
                    begin,
                    tag,
                    slice,
                    |cursor, &val| {
                        cursor.write_unaligned(val);
                    },
                );
            }
            FieldKind::RepeatedFixed32 => {
                let slice = obj_state.get_slice::<u32>(offset);
                write_repeated(
                    &mut obj_state,
                    &mut cursor,
                    begin,
                    tag,
                    slice,
                    |cursor, &val| {
                        cursor.write_unaligned(val);
                    },
                );
            }
            FieldKind::RepeatedBytes => {
                let slice = obj_state.get_slice::<Bytes>(offset);
                if obj_state.rep_field_idx == 0 {
                    obj_state.rep_field_idx = slice.len();
                }
                while obj_state.rep_field_idx > 0 {
                    if cursor <= begin {
                        break;
                    }
                    obj_state.rep_field_idx -= 1;
                    let bytes = slice[obj_state.rep_field_idx].as_ref();
                    let len = bytes.len();
                    // We don't use slop as we need to write length prefix and tag too.
                    let buffer_size = (cursor - begin) as usize;
                    if buffer_size < len {
                        cursor.write_slice(&bytes[len - buffer_size..]);
                        obj_state.push(tag, count(cursor, begin, byte_count), stack)?;
                        return Some((cursor, EncodeObject::String(&bytes[..len - buffer_size])));
                    }
                    cursor.write_slice(bytes);
                    cursor.write_varint(bytes.len() as u64);
                    cursor.write_tag(tag);
                }
            }
            FieldKind::RepeatedMessage => {
                let (offset, child_table) = aux_entry(offset, obj_state.table);
                let slice = obj_state.get_slice::<*const Object>(offset);
                if obj_state.rep_field_idx == 0 {
                    obj_state.rep_field_idx = slice.len();
                }
                if obj_state.rep_field_idx > 0 {
                    obj_state.rep_field_idx -= 1;
                    obj_state.push(tag, count(cursor, begin, byte_count), stack)?;
                    obj_state = ObjectEncodeState::new(
                        unsafe { &*slice[obj_state.rep_field_idx] },
                        child_table,
                    );
                }
            }
            FieldKind::RepeatedGroup => {
                let (offset, child_table) = aux_entry(offset, obj_state.table);
                let slice = obj_state.get_slice::<*const Object>(offset);
                if obj_state.rep_field_idx == 0 {
                    obj_state.rep_field_idx = slice.len();
                }
                if obj_state.rep_field_idx > 0 {
                    if cursor <= begin {
                        break;
                    }
                    obj_state.rep_field_idx -= 1;
                    let mut end_tag = tag;
                    end_tag += 1; // Set wire type to END_GROUP
                    cursor.write_tag(end_tag);
                    obj_state.push(tag, -1, stack)?;
                    obj_state = ObjectEncodeState::new(
                        unsafe { &*slice[obj_state.rep_field_idx] },
                        child_table,
                    );
                }
            }
        }
    }
    Some((cursor, EncodeObject::Object(obj_state)))
}

struct ResumableState<'a> {
    object: EncodeObject<'a>,
    overrun: isize,
    byte_count: isize,
}

impl<'a> ResumableState<'a> {
    fn go_encode(self, buffer: &mut [u8], stack: &mut Stack<StackEntry>) -> Option<Self> {
        let len = buffer.len() as isize;
        let ResumableState {
            object,
            overrun,
            mut byte_count,
        } = self;
        byte_count += len;
        assert!(self.overrun <= 0 && self.overrun >= -(SLOP_SIZE as isize));
        if self.overrun + len > 0 {
            let (mut cursor, begin) = WriteCursor::new(buffer);
            cursor += overrun;
            let (new_cursor, object) = match object {
                EncodeObject::Done => (cursor, EncodeObject::Done),
                EncodeObject::Object(ctx) => encode_loop(ctx, cursor, begin, byte_count, stack)?,
                EncodeObject::String(bytes) => {
                    encode_bytes(bytes, cursor, begin, byte_count, stack)?
                }
            };
            Some(ResumableState {
                object,
                byte_count,
                overrun: new_cursor - begin,
            })
        } else {
            Some(ResumableState {
                object,
                overrun: overrun + len,
                byte_count,
            })
        }
    }
}

pub(crate) struct ResumeableEncode<'a, const STACK_DEPTH: usize> {
    state: MaybeUninit<ResumableState<'a>>,
    patch_buffer: [u8; 2 * SLOP_SIZE],
    stack: StackWithStorage<StackEntry, STACK_DEPTH>,
}

pub(crate) enum ResumeResult<'a> {
    Done(&'a [u8]),
    NeedsMoreBuffer,
}

impl<'a, const STACK_DEPTH: usize> ResumeableEncode<'a, STACK_DEPTH> {
    pub(crate) fn new<T: Protobuf + ?Sized>(obj: &'a T) -> Self {
        let table = T::encoding_table();
        let encode_ctx = ObjectEncodeState {
            obj: obj.as_object(),
            table,
            field_idx: table.len(),
            rep_field_idx: 0,
        };
        Self {
            state: MaybeUninit::new(ResumableState {
                overrun: 0,
                object: EncodeObject::Object(encode_ctx),
                byte_count: 0,
            }),
            patch_buffer: [0; 2 * SLOP_SIZE],
            stack: Default::default(),
        }
    }

    pub(crate) fn resume_encode<'b>(&mut self, buffer: &'b mut [u8]) -> Option<ResumeResult<'b>> {
        let len = buffer.len() as isize;
        let mut state = unsafe { self.state.assume_init_read() };
        if len > SLOP_SIZE as isize {
            buffer[len as usize - SLOP_SIZE..].copy_from_slice(&self.patch_buffer[..SLOP_SIZE]);
            state = state.go_encode(&mut buffer[SLOP_SIZE..], &mut self.stack)?;
            if matches!(state.object, EncodeObject::Done) {
                // Leave in uninitialized state to prevent further use
                return Some(ResumeResult::Done(
                    &buffer[(SLOP_SIZE as isize + state.overrun) as usize..],
                ));
            }
            self.patch_buffer[SLOP_SIZE..].copy_from_slice(&buffer[..SLOP_SIZE]);
            state = state.go_encode(&mut self.patch_buffer[SLOP_SIZE..], &mut self.stack)?;
            buffer[..SLOP_SIZE].copy_from_slice(&self.patch_buffer[SLOP_SIZE..]);
            if matches!(state.object, EncodeObject::Done) && state.overrun >= 0 {
                // Finished and still in this buffer
                return Some(ResumeResult::Done(&buffer[state.overrun as usize..]));
            }
        } else {
            self.patch_buffer.copy_within(..SLOP_SIZE, len as usize);
            state = state.go_encode(
                &mut self.patch_buffer[SLOP_SIZE..SLOP_SIZE + len as usize],
                &mut self.stack,
            )?;
            buffer.copy_from_slice(&self.patch_buffer[SLOP_SIZE..SLOP_SIZE + len as usize]);
            if matches!(state.object, EncodeObject::Done) && state.overrun >= 0 {
                return Some(ResumeResult::Done(&buffer[state.overrun as usize..]));
            }
        }
        self.state.write(state);
        Some(ResumeResult::NeedsMoreBuffer)
    }
}
