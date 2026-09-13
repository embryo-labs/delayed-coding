#![cfg(feature = "log-schedule")]
use delayed_coding_ffi::{dc_log_decode, dc_log_encode, dc_log_model_free, dc_log_model_new};
use delayed_coding_ffi::{dc_workspace_free, dc_workspace_new, DcStatus};
use std::ptr;

#[test]
fn optional_log_handles_and_boundaries() {
    unsafe {
        let mut model = ptr::null_mut();
        let mut workspace = ptr::null_mut();
        assert_eq!(
            dc_log_model_new([32768, 0, 32768].as_ptr(), 3, &mut model),
            DcStatus::Ok
        );
        assert_eq!(dc_workspace_new(&mut workspace), DcStatus::Ok);
        for lanes in [1, 2, 4, 8] {
            let input = [0, 2, 0, 2, 2, 0, 0, 2, 2];
            let mut storage = [0xaa; 32];
            let mut offset = 0;
            let mut size = 0;
            assert_eq!(
                dc_log_encode(
                    model,
                    lanes,
                    input.as_ptr(),
                    input.len(),
                    storage.as_mut_ptr(),
                    storage.len(),
                    workspace,
                    &mut offset,
                    &mut size
                ),
                DcStatus::Ok
            );
            assert!(storage[..offset].iter().all(|&b| b == 0xaa));
            let mut out = [0; 9];
            assert_eq!(
                dc_log_decode(
                    model,
                    lanes,
                    storage[offset..].as_ptr(),
                    size,
                    out.as_mut_ptr(),
                    out.len()
                ),
                DcStatus::Ok
            );
            assert_eq!(input, out);
            assert_eq!(
                dc_log_decode(
                    model,
                    lanes,
                    storage[offset..].as_ptr(),
                    size - 1,
                    out.as_mut_ptr(),
                    out.len()
                ),
                DcStatus::TruncatedInput
            );
            assert_eq!(
                dc_log_encode(
                    model,
                    lanes,
                    ptr::null(),
                    0,
                    ptr::null_mut(),
                    0,
                    workspace,
                    &mut offset,
                    &mut size
                ),
                DcStatus::Ok
            );
            assert_eq!((offset, size), (0, 0));
            assert_eq!(
                dc_log_decode(model, lanes, ptr::null(), 0, ptr::null_mut(), 0),
                DcStatus::Ok
            );
        }
        let mut offset = 77;
        let mut size = 88;
        let mut output = [0xaa; 8];
        assert_eq!(
            dc_log_encode(
                model,
                4,
                [0, 2, 256].as_ptr(),
                3,
                output.as_mut_ptr(),
                output.len(),
                workspace,
                &mut offset,
                &mut size
            ),
            DcStatus::InvalidSymbol
        );
        assert_eq!((offset, size), (77, 88));
        assert_eq!(output, [0xaa; 8]);
        assert_eq!(
            dc_log_decode(model, 3, ptr::null(), 0, ptr::null_mut(), 0),
            DcStatus::InvalidArgument
        );
        assert_eq!(
            dc_log_decode(ptr::null(), 4, ptr::null(), 0, ptr::null_mut(), 0),
            DcStatus::InvalidArgument
        );
        dc_log_model_free(model);
        dc_workspace_free(workspace);
        dc_log_model_free(ptr::null_mut());
    }
}
