use std::{mem::MaybeUninit, ptr::NonNull};

use crate::{
    Protobuf,
    base::Object,
    decoding::FieldKind,
    repeated_field::{Bytes, RepeatedField},
    utils::{Stack, StackWithStorage},
    wire::{SLOP_SIZE, WriteCursor, zigzag_encode},
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
    index: usize,
    byte_count: isize,
    tag: u32,
}

impl StackEntry {
    fn to_context<'a>(self) -> (EncodeContext<'a>, u32, isize) {
        (
            EncodeContext {
                obj: unsafe { &*self.obj },
                table: unsafe { &*self.table },
                index: self.index,
            },
            self.tag,
            self.byte_count,
        )
    }
}

enum EncodeObject<'a> {
    Done,
    Object(&'a Object, &'a [TableEntry], usize),
    String(&'a [u8]),
}

struct EncodeContext<'a> {
    obj: &'a Object,
    table: &'a [TableEntry],
    index: usize,
}

impl EncodeContext<'_> {
    fn push(&mut self, tag: u32, byte_count: isize, stack: &mut Stack<StackEntry>) -> Option<()> {
        stack.push(StackEntry {
            obj: self.obj,
            table: self.table,
            index: self.index,
            tag,
            byte_count,
        })?;
        Some(())
    }

    fn pop(&mut self, stack: &mut Stack<StackEntry>) -> Option<(u32, isize)> {
        let (ctx, tag, byte_count) = stack.pop()?.to_context();
        *self = ctx;
        Some((tag, byte_count))
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
    let (ctx, tag, old_byte_count) = stack.pop()?.to_context();
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

fn encode_loop<'a>(
    mut ctx: EncodeContext<'a>,
    mut cursor: WriteCursor,
    begin: NonNull<u8>,
    byte_count: isize,
    stack: &mut Stack<StackEntry>,
) -> EncodeResult<'a> {
    loop {
        while ctx.index >= ctx.table.len() {
            if cursor <= begin {
                break;
            }
            let Some((tag, old_byte_count)) = ctx.pop(stack) else {
                return Some((cursor, EncodeObject::Done));
            };
            if old_byte_count >= 0 {
                let field_byte_count = count(cursor, begin, byte_count) - old_byte_count;
                cursor.write_varint(field_byte_count as u64);
            }
            cursor.write_tag(tag);
        }
        let TableEntry {
            has_bit,
            kind,
            offset,
            encoded_tag: tag,
        } = ctx.table[ctx.index];
        ctx.index += 1;
        let offset = offset as usize;
        match kind {
            FieldKind::Unknown => {
                unreachable!()
            }
            FieldKind::Varint64 => {
                if ctx.obj.has_bit(has_bit) {
                    if cursor <= begin {
                        break;
                    }
                    cursor.write_varint(ctx.obj.get::<u64>(offset));
                    cursor.write_tag(tag);
                }
            }
            FieldKind::Varint32 => {
                if ctx.obj.has_bit(has_bit) {
                    if cursor <= begin {
                        break;
                    }
                    cursor.write_varint(ctx.obj.get::<u32>(offset) as u64);
                    cursor.write_tag(tag);
                }
            }
            FieldKind::Varint64Zigzag => {
                if ctx.obj.has_bit(has_bit) {
                    if cursor <= begin {
                        break;
                    }
                    cursor.write_varint(zigzag_encode(ctx.obj.get::<i64>(offset)));
                    cursor.write_tag(tag);
                }
            }
            FieldKind::Varint32Zigzag => {
                if ctx.obj.has_bit(has_bit) {
                    if cursor <= begin {
                        break;
                    }
                    cursor.write_varint(zigzag_encode(ctx.obj.get::<i32>(offset) as i64));
                    cursor.write_tag(tag);
                }
            }
            FieldKind::Fixed64 => {
                if ctx.obj.has_bit(has_bit) {
                    if cursor <= begin {
                        break;
                    }
                    cursor.write_unaligned(ctx.obj.get::<u64>(offset));
                    cursor.write_tag(tag);
                }
            }
            FieldKind::Fixed32 => {
                if ctx.obj.has_bit(has_bit) {
                    if cursor <= begin {
                        break;
                    }
                    cursor.write_unaligned(ctx.obj.get::<u32>(offset));
                    cursor.write_tag(tag);
                }
            }
            FieldKind::Bytes => {
                if ctx.obj.has_bit(has_bit) {
                    if cursor <= begin {
                        break;
                    }
                    let bytes = ctx.obj.ref_at::<RepeatedField<u8>>(offset).as_ref();
                    let len = bytes.len();
                    // We don't use slop as we need to write length prefix and tag too.
                    let buffer_size = (cursor - begin) as usize;
                    if buffer_size < len {
                        cursor.write_slice(&bytes[len - buffer_size..]);
                        ctx.push(tag, count(cursor, begin, byte_count), stack)?;
                        return Some((cursor, EncodeObject::String(&bytes[..len - buffer_size])));
                    }
                    cursor.write_slice(bytes);
                    cursor.write_varint(bytes.len() as u64);
                    cursor.write_tag(tag);
                }
            }
            FieldKind::Message => {
                let (offset, child_table) = aux_entry(offset, ctx.table);
                let child_ptr = ctx.obj.get::<*const Object>(offset);
                if !child_ptr.is_null() {
                    if cursor <= begin {
                        break;
                    }
                    ctx.push(tag, count(cursor, begin, byte_count), stack)?;
                    ctx.obj = unsafe { &*child_ptr };
                    ctx.table = child_table;
                    ctx.index = 0;
                    continue;
                }
            }
            FieldKind::Group => {
                let (offset, child_table) = aux_entry(offset, ctx.table);
                let child_ptr = ctx.obj.get::<*const Object>(offset);
                if !child_ptr.is_null() {
                    if cursor <= begin {
                        break;
                    }
                    let mut end_tag = tag;
                    end_tag += 1 << 24; // Set wire type to END_GROUP
                    cursor.write_tag(end_tag);
                    ctx.push(tag, -1, stack)?;
                    ctx.obj = unsafe { &*child_ptr };
                    ctx.table = child_table;
                    ctx.index = 0;
                }
            }
            FieldKind::RepeatedVarint64 => {
                for &val in ctx.obj.ref_at::<RepeatedField<u64>>(offset).as_ref() {
                    if cursor <= begin {
                        break;
                    }
                    cursor.write_varint(val);
                    cursor.write_tag(tag);
                }
            }
            FieldKind::RepeatedVarint32 => {
                for &val in ctx.obj.ref_at::<RepeatedField<u32>>(offset).as_ref() {
                    if cursor <= begin {
                        break;
                    }
                    cursor.write_varint(val as u64);
                    cursor.write_tag(tag);
                }
            }
            FieldKind::RepeatedVarint64Zigzag => {
                for &val in ctx.obj.ref_at::<RepeatedField<i64>>(offset).as_ref() {
                    if cursor <= begin {
                        break;
                    }
                    cursor.write_varint(zigzag_encode(val));
                    cursor.write_tag(tag);
                }
            }
            FieldKind::RepeatedVarint32Zigzag => {
                for &val in ctx.obj.ref_at::<RepeatedField<i32>>(offset).as_ref() {
                    if cursor <= begin {
                        break;
                    }
                    cursor.write_varint(zigzag_encode(val as i64));
                    cursor.write_tag(tag);
                }
            }
            FieldKind::RepeatedFixed64 => {
                for &val in ctx.obj.ref_at::<RepeatedField<u64>>(offset).as_ref() {
                    if cursor <= begin {
                        break;
                    }
                    cursor.write_unaligned(val);
                    cursor.write_tag(tag);
                }
            }
            FieldKind::RepeatedFixed32 => {
                for &val in ctx.obj.ref_at::<RepeatedField<u32>>(offset).as_ref() {
                    if cursor <= begin {
                        break;
                    }
                    cursor.write_unaligned(val);
                    cursor.write_tag(tag);
                }
            }
            FieldKind::RepeatedBytes => {
                for bytes in ctx.obj.ref_at::<RepeatedField<Bytes>>(offset).as_ref() {
                    if cursor <= begin {
                        break;
                    }
                    if cursor - begin + (bytes.len() as isize) > SLOP_SIZE as isize {
                        unimplemented!();
                    }
                    cursor.write_slice(bytes);
                    cursor.write_varint(bytes.len() as u64);
                    cursor.write_tag(tag);
                }
            }
            FieldKind::RepeatedMessage => {
                let (offset, child_table) = aux_entry(offset, ctx.table);
                for &message_ptr in ctx
                    .obj
                    .ref_at::<RepeatedField<*const Object>>(offset)
                    .as_ref()
                {
                    ctx.push(tag, count(cursor, begin, byte_count), stack)?;
                    ctx.obj = unsafe { &*message_ptr };
                    ctx.table = child_table;
                    ctx.index = 0;
                }
            }
            FieldKind::RepeatedGroup => {
                let (offset, child_table) = aux_entry(offset, ctx.table);
                for &message_ptr in ctx
                    .obj
                    .ref_at::<RepeatedField<*const Object>>(offset)
                    .as_ref()
                {
                    if cursor <= begin {
                        break;
                    }
                    let mut end_tag = tag;
                    end_tag += 1 << 24; // Set wire type to END_GROUP
                    cursor.write_tag(end_tag);
                    ctx.push(tag, -1, stack)?;
                    ctx.obj = unsafe { &*message_ptr };
                    ctx.table = child_table;
                    ctx.index = 0;
                }
            }
        }
    }
    Some((cursor, EncodeObject::Object(ctx.obj, ctx.table, ctx.index)))
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
                EncodeObject::Object(obj, table, index) => {
                    let ctx = EncodeContext { obj, table, index };
                    encode_loop(ctx, cursor, begin, byte_count, stack)?
                }
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

struct ResumeableEncode<'a, const STACK_DEPTH: usize> {
    state: MaybeUninit<ResumableState<'a>>,
    patch_buffer: [u8; 2 * SLOP_SIZE],
    stack: StackWithStorage<StackEntry, STACK_DEPTH>,
}

enum ResumeResult<'a> {
    Done(&'a [u8]),
    NeedsMoreBuffer,
}

impl<'a, const STACK_DEPTH: usize> ResumeableEncode<'a, STACK_DEPTH> {
    pub fn new<T: Protobuf>(obj: &'a T) -> Self {
        Self {
            state: MaybeUninit::new(ResumableState {
                overrun: 0,
                object: EncodeObject::Object(obj.as_object(), T::encoding_table(), 0),
                byte_count: 0,
            }),
            patch_buffer: [0; 2 * SLOP_SIZE],
            stack: Default::default(),
        }
    }

    pub fn resume_encode<'b>(&mut self, buffer: &'b mut [u8]) -> Option<ResumeResult<'b>> {
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

pub fn encode_flat<'a, const STACK_DEPTH: usize>(
    obj: &impl Protobuf,
    buffer: &'a mut [u8],
) -> anyhow::Result<&'a [u8]> {
    let mut resumeable_encode = ResumeableEncode::<STACK_DEPTH>::new(obj);
    let ResumeResult::Done(buf) = resumeable_encode
        .resume_encode(buffer)
        .ok_or(anyhow::anyhow!("Message tree too deep"))?
    else {
        return Err(anyhow::anyhow!("Buffer too small for message"));
    };
    Ok(buf)
}
