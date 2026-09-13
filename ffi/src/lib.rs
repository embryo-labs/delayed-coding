//! C ABI boundary. The codec itself forbids unsafe code. See include/delayed_coding.h.
#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(feature = "log-schedule")]
mod log_schedule;
#[cfg(feature = "log-schedule")]
pub use log_schedule::{dc_log_decode, dc_log_encode, dc_log_model_free, dc_log_model_new};

use delayed_coding::decode_branchless4_into;
use delayed_coding::decode_grouped4_into;
use delayed_coding::decode_lookahead_interleaved_into;
use delayed_coding::{decode_interleaved_into, encode_interleaved_into};
use delayed_coding::{
    decode_into, decode_lookahead_into, encode_into, Error, Model, TableOptions, Workspace,
};
use delayed_coding::{encode_branches_into, Branch, Interval};
use std::panic::{catch_unwind, AssertUnwindSafe};

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DcStatus {
    Ok = 0,
    InvalidArgument = 1,
    InvalidModel = 2,
    InvalidSymbol = 3,
    InvalidDelay = 4,
    InputTooLarge = 5,
    OutputTooSmall = 6,
    TruncatedInput = 7,
    TrailingInput = 8,
    InvalidState = 9,
    Panic = 10,
}

impl From<Error> for DcStatus {
    fn from(error: Error) -> Self {
        match error {
            Error::InvalidModel => Self::InvalidModel,
            Error::InvalidSymbol => Self::InvalidSymbol,
            Error::InvalidDelay => Self::InvalidDelay,
            Error::InputTooLarge => Self::InputTooLarge,
            Error::OutputTooSmall => Self::OutputTooSmall,
            Error::TruncatedInput => Self::TruncatedInput,
            Error::TrailingInput => Self::TrailingInput,
            Error::InvalidState => Self::InvalidState,
            _ => Self::InvalidArgument,
        }
    }
}

fn guard(f: impl FnOnce() -> Result<(), DcStatus>) -> DcStatus {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(())) => DcStatus::Ok,
        Ok(Err(error)) => error,
        Err(_) => DcStatus::Panic,
    }
}

fn valid_slice<T>(pointer: *const T, length: usize) -> bool {
    length == 0
        || (!pointer.is_null()
            && (pointer as usize).is_multiple_of(std::mem::align_of::<T>())
            && length <= isize::MAX as usize / std::mem::size_of::<T>())
}

/// # Safety
/// Intervals describes count readable, aligned entries. Out is writable, aligned
/// and disjoint. The mapping is copied; input can be released on return.
#[no_mangle]
pub unsafe extern "C" fn dc_branch_new(
    intervals: *const Interval,
    count: usize,
    frequency: u32,
    out: *mut *mut Branch,
) -> DcStatus {
    guard(|| {
        if !valid_slice(intervals, count) || !valid_slice(out, 1) {
            return Err(DcStatus::InvalidArgument);
        }
        unsafe {
            *out = std::ptr::null_mut();
        }
        let intervals = if count == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(intervals, count) }
        };
        let branch = Branch::new(intervals).map_err(DcStatus::from)?;
        if branch.frequency() != frequency {
            return Err(DcStatus::InvalidModel);
        }
        unsafe {
            *out = Box::into_raw(Box::new(branch));
        }
        Ok(())
    })
}

/// # Safety
/// Branch must be null or an unreleased handle returned by dc_branch_new, with no live users.
#[no_mangle]
pub unsafe extern "C" fn dc_branch_free(branch: *mut Branch) {
    if !branch.is_null() {
        unsafe {
            drop(Box::from_raw(branch));
        }
    }
}

/// # Safety
/// Branches is an array of count live immutable branch handles. Workspace is
/// exclusively borrowed. Output is writable and disjoint from inputs/handles;
/// offset/size are writable, aligned and disjoint from each other and all buffers.
/// Every borrowed allocation/handle remains valid and immutable for the call.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dc_encode_branches(
    branches: *const *const Branch,
    count: usize,
    delay: u32,
    lanes: u32,
    output: *mut u8,
    capacity: usize,
    workspace: *mut Workspace,
    offset: *mut usize,
    size: *mut usize,
) -> DcStatus {
    guard(|| {
        if !valid_slice(branches, count)
            || !valid_slice(output, capacity)
            || !valid_slice(workspace, 1)
            || !valid_slice(offset, 1)
            || !valid_slice(size, 1)
        {
            return Err(DcStatus::InvalidArgument);
        }
        let handles = if count == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(branches, count) }
        };
        if handles.iter().any(|&p| !valid_slice(p, 1)) {
            return Err(DcStatus::InvalidArgument);
        }
        let branches = handles.iter().map(|&p| unsafe { &*p });
        let output = if capacity == 0 {
            &mut []
        } else {
            unsafe { std::slice::from_raw_parts_mut(output, capacity) }
        };
        let workspace = unsafe { &mut *workspace };
        if !matches!(lanes, 1 | 4) {
            return Err(DcStatus::InvalidArgument);
        }
        let range = match (delay, lanes) {
            (16, 1) => encode_branches_into::<16, 1>(branches, output, workspace),
            (24, 1) => encode_branches_into::<24, 1>(branches, output, workspace),
            (32, 1) => encode_branches_into::<32, 1>(branches, output, workspace),
            (16, 4) => encode_branches_into::<16, 4>(branches, output, workspace),
            (24, 4) => encode_branches_into::<24, 4>(branches, output, workspace),
            (32, 4) => encode_branches_into::<32, 4>(branches, output, workspace),
            _ => return Err(DcStatus::InvalidDelay),
        }
        .map_err(DcStatus::from)?;
        unsafe {
            *offset = range.start;
            *size = range.len();
        }
        Ok(())
    })
}

/// # Safety
/// Frequencies must point to count readable u32s. Out must be valid, writable,
/// aligned and disjoint. On success the caller owns the returned model handle.
#[no_mangle]
pub unsafe extern "C" fn dc_model_new(
    frequencies: *const u32,
    count: usize,
    out: *mut *mut Model,
) -> DcStatus {
    unsafe { dc_model_new_with_options(frequencies, count, 0, out) }
}

/// # Safety
/// Same pointer contract as dc_model_new. Flags: 1=direct encode, 2=direct decode.
#[no_mangle]
pub unsafe extern "C" fn dc_model_new_with_options(
    frequencies: *const u32,
    count: usize,
    flags: u32,
    out: *mut *mut Model,
) -> DcStatus {
    guard(|| {
        if !valid_slice(frequencies, count) || out.is_null() || flags & !3 != 0 {
            return Err(DcStatus::InvalidArgument);
        }
        unsafe {
            *out = std::ptr::null_mut();
        }
        let frequencies = if count == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(frequencies, count) }
        };
        let model = Model::new(frequencies)
            .map_err(DcStatus::from)?
            .with_tables(TableOptions {
                direct_encode: flags & 1 != 0,
                direct_decode: flags & 2 != 0,
            });
        unsafe {
            *out = Box::into_raw(Box::new(model));
        }
        Ok(())
    })
}

/// # Safety
/// Model must be null or an unreleased handle from dc_model_new, with no live users.
#[no_mangle]
pub unsafe extern "C" fn dc_model_free(model: *mut Model) {
    if !model.is_null() {
        unsafe {
            drop(Box::from_raw(model));
        }
    }
}

/// # Safety
/// Out must point to one writable, aligned handle slot.
#[no_mangle]
pub unsafe extern "C" fn dc_workspace_new(out: *mut *mut Workspace) -> DcStatus {
    guard(|| {
        if out.is_null() {
            return Err(DcStatus::InvalidArgument);
        }
        unsafe {
            *out = Box::into_raw(Box::<Workspace>::default());
        }
        Ok(())
    })
}

/// # Safety
/// Workspace must be null or an unreleased handle from dc_workspace_new with no live users.
#[no_mangle]
pub unsafe extern "C" fn dc_workspace_free(workspace: *mut Workspace) {
    if !workspace.is_null() {
        unsafe {
            drop(Box::from_raw(workspace));
        }
    }
}

/// # Safety
/// Handles must be valid; workspace is exclusively borrowed during this call.
/// Input/output pointers must describe valid, aligned, nonoverlapping buffers.
/// Offset/size must be writable and disjoint from each other and every buffer/handle.
#[no_mangle]
pub unsafe extern "C" fn dc_encode(
    model: *const Model,
    delay: u32,
    symbols: *const u32,
    count: usize,
    output: *mut u8,
    capacity: usize,
    workspace: *mut Workspace,
    offset: *mut usize,
    size: *mut usize,
) -> DcStatus {
    unsafe {
        dc_encode_interleaved(
            model, delay, 1, symbols, count, output, capacity, workspace, offset, size,
        )
    }
}

/// # Safety
/// Same buffer/handle contract as dc_encode. Lanes must be 1 or 4.
#[no_mangle]
pub unsafe extern "C" fn dc_encode_interleaved(
    model: *const Model,
    delay: u32,
    lanes: u32,
    symbols: *const u32,
    count: usize,
    output: *mut u8,
    capacity: usize,
    workspace: *mut Workspace,
    offset: *mut usize,
    size: *mut usize,
) -> DcStatus {
    guard(|| {
        if model.is_null()
            || workspace.is_null()
            || offset.is_null()
            || size.is_null()
            || !valid_slice(symbols, count)
            || !valid_slice(output, capacity)
        {
            return Err(DcStatus::InvalidArgument);
        }
        let model = unsafe { &*model };
        let workspace = unsafe { &mut *workspace };
        let symbols = if count == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(symbols, count) }
        };
        let output = if capacity == 0 {
            &mut []
        } else {
            unsafe { std::slice::from_raw_parts_mut(output, capacity) }
        };
        if !matches!(lanes, 1 | 4) {
            return Err(DcStatus::InvalidArgument);
        }
        let result = match (delay, lanes) {
            (16, 1) => encode_into::<16>(model, symbols, output, workspace),
            (24, 1) => encode_into::<24>(model, symbols, output, workspace),
            (32, 1) => encode_into::<32>(model, symbols, output, workspace),
            (16, 4) => encode_interleaved_into::<16, 4>(model, symbols, output, workspace),
            (24, 4) => encode_interleaved_into::<24, 4>(model, symbols, output, workspace),
            (32, 4) => encode_interleaved_into::<32, 4>(model, symbols, output, workspace),
            _ => return Err(DcStatus::InvalidDelay),
        }
        .map_err(DcStatus::from)?;
        unsafe {
            *offset = result.start;
            *size = result.len();
        }
        Ok(())
    })
}

/// # Safety
/// Model must be a valid live handle. Input/output must be valid, aligned,
/// nonoverlapping buffers; output must be exclusively writable for count u32s.
#[no_mangle]
pub unsafe extern "C" fn dc_decode(
    model: *const Model,
    delay: u32,
    input: *const u8,
    size: usize,
    output: *mut u32,
    count: usize,
) -> DcStatus {
    unsafe { dc_decode_interleaved(model, delay, 1, input, size, output, count) }
}

/// # Safety
/// Same buffer/handle contract as dc_decode. Lanes must match encoding (1 or 4).
#[no_mangle]
pub unsafe extern "C" fn dc_decode_interleaved(
    model: *const Model,
    delay: u32,
    lanes: u32,
    input: *const u8,
    size: usize,
    output: *mut u32,
    count: usize,
) -> DcStatus {
    unsafe { decode_dispatch(model, delay, lanes, input, size, output, count, 0) }
}

/// # Safety
/// Same pointer contract as dc_decode. Experimental fixed-model, single-lane path.
#[no_mangle]
pub unsafe extern "C" fn dc_decode_lookahead(
    model: *const Model,
    delay: u32,
    input: *const u8,
    size: usize,
    output: *mut u32,
    count: usize,
) -> DcStatus {
    unsafe { decode_dispatch(model, delay, 1, input, size, output, count, 1) }
}

/// # Safety
/// Same pointer contract as dc_decode_interleaved. Lanes must be 1 or 4.
#[no_mangle]
pub unsafe extern "C" fn dc_decode_lookahead_interleaved(
    model: *const Model,
    delay: u32,
    lanes: u32,
    input: *const u8,
    size: usize,
    output: *mut u32,
    count: usize,
) -> DcStatus {
    unsafe { decode_dispatch(model, delay, lanes, input, size, output, count, 1) }
}

/// # Safety
/// Same pointer contract as dc_decode_interleaved, with exactly four states.
#[no_mangle]
pub unsafe extern "C" fn dc_decode_grouped4(
    model: *const Model,
    delay: u32,
    input: *const u8,
    size: usize,
    output: *mut u32,
    count: usize,
) -> DcStatus {
    unsafe { decode_dispatch(model, delay, 4, input, size, output, count, 2) }
}

/// # Safety
/// Same pointer contract as dc_decode_interleaved, with exactly four states.
/// No padding beyond the specified input slice is required.
#[no_mangle]
pub unsafe extern "C" fn dc_decode_branchless4(
    model: *const Model,
    delay: u32,
    input: *const u8,
    size: usize,
    output: *mut u32,
    count: usize,
) -> DcStatus {
    unsafe { decode_dispatch(model, delay, 4, input, size, output, count, 3) }
}

#[allow(clippy::too_many_arguments)]
unsafe fn decode_dispatch(
    model: *const Model,
    delay: u32,
    lanes: u32,
    input: *const u8,
    size: usize,
    output: *mut u32,
    count: usize,
    mode: u8,
) -> DcStatus {
    guard(|| {
        if model.is_null() || !valid_slice(input, size) || !valid_slice(output, count) {
            return Err(DcStatus::InvalidArgument);
        }
        let model = unsafe { &*model };
        let input = if size == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(input, size) }
        };
        let output = if count == 0 {
            &mut []
        } else {
            unsafe { std::slice::from_raw_parts_mut(output, count) }
        };
        if !matches!(lanes, 1 | 4) {
            return Err(DcStatus::InvalidArgument);
        }
        if mode == 3 {
            return match delay {
                16 => decode_branchless4_into::<16>(model, input, output),
                24 => decode_branchless4_into::<24>(model, input, output),
                32 => decode_branchless4_into::<32>(model, input, output),
                _ => return Err(DcStatus::InvalidDelay),
            }
            .map_err(DcStatus::from);
        }
        if mode == 2 {
            return match delay {
                16 => decode_grouped4_into::<16>(model, input, output),
                24 => decode_grouped4_into::<24>(model, input, output),
                32 => decode_grouped4_into::<32>(model, input, output),
                _ => return Err(DcStatus::InvalidDelay),
            }
            .map_err(DcStatus::from);
        }
        if mode == 1 {
            return match (delay, lanes) {
                (16, 1) => decode_lookahead_into::<16>(model, input, output),
                (24, 1) => decode_lookahead_into::<24>(model, input, output),
                (32, 1) => decode_lookahead_into::<32>(model, input, output),
                (16, 4) => decode_lookahead_interleaved_into::<16, 4>(model, input, output),
                (24, 4) => decode_lookahead_interleaved_into::<24, 4>(model, input, output),
                (32, 4) => decode_lookahead_interleaved_into::<32, 4>(model, input, output),
                _ => return Err(DcStatus::InvalidDelay),
            }
            .map_err(DcStatus::from);
        }
        match (delay, lanes) {
            (16, 1) => decode_into::<16>(model, input, output),
            (24, 1) => decode_into::<24>(model, input, output),
            (32, 1) => decode_into::<32>(model, input, output),
            (16, 4) => decode_interleaved_into::<16, 4>(model, input, output),
            (24, 4) => decode_interleaved_into::<24, 4>(model, input, output),
            (32, 4) => decode_interleaved_into::<32, 4>(model, input, output),
            _ => return Err(DcStatus::InvalidDelay),
        }
        .map_err(DcStatus::from)
    })
}
