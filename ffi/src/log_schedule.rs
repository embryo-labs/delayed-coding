//! Separate, opt-in logarithmic DC format; never dispatched by the legacy API.
use super::{guard, valid_slice, DcStatus, Workspace};
use delayed_coding::LogModel;

/// # Safety
/// Frequencies is a readable array; out is aligned, writable and disjoint.
#[no_mangle]
pub unsafe extern "C" fn dc_log_model_new(
    frequencies: *const u32,
    count: usize,
    out: *mut *mut LogModel,
) -> DcStatus {
    guard(|| {
        if !valid_slice(frequencies, count) || !valid_slice(out, 1) {
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
        let model = LogModel::new(frequencies).map_err(DcStatus::from)?;
        unsafe {
            *out = Box::into_raw(Box::new(model));
        }
        Ok(())
    })
}

/// # Safety
/// Model is null or an unreleased dc_log_model_new handle with no live users.
#[no_mangle]
pub unsafe extern "C" fn dc_log_model_free(model: *mut LogModel) {
    if !model.is_null() {
        unsafe {
            drop(Box::from_raw(model));
        }
    }
}

/// # Safety
/// Same disjoint, aligned buffer/metadata and exclusive workspace contract as
/// dc_encode; model must be a live dc_log_model_new handle.
#[no_mangle]
pub unsafe extern "C" fn dc_log_encode(
    model: *const LogModel,
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
        if !valid_slice(model, 1)
            || !valid_slice(workspace, 1)
            || !valid_slice(offset, 1)
            || !valid_slice(size, 1)
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
        let range = match lanes {
            1 => model.encode_into::<1>(symbols, output, workspace),
            2 => model.encode_into::<2>(symbols, output, workspace),
            4 => model.encode_into::<4>(symbols, output, workspace),
            8 => model.encode_into::<8>(symbols, output, workspace),
            _ => return Err(DcStatus::InvalidArgument),
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
/// Model is a live immutable log model; input/output are readable/writable,
/// aligned, disjoint buffers valid for the specified lengths. No padding needed.
#[no_mangle]
pub unsafe extern "C" fn dc_log_decode(
    model: *const LogModel,
    lanes: u32,
    input: *const u8,
    size: usize,
    output: *mut u32,
    count: usize,
) -> DcStatus {
    guard(|| {
        if !valid_slice(model, 1) || !valid_slice(input, size) || !valid_slice(output, count) {
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
        match lanes {
            1 => model.decode_into::<1>(input, output),
            2 => model.decode_into::<2>(input, output),
            4 => model.decode_into::<4>(input, output),
            8 => model.decode_into::<8>(input, output),
            _ => return Err(DcStatus::InvalidArgument),
        }
        .map_err(DcStatus::from)
    })
}
