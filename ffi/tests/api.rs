use delayed_coding_ffi::*;
use std::ptr::{null, null_mut};

#[test]
fn branch_handles_and_batch_encoding() {
    unsafe {
        let mut branch = null_mut();
        let interval = delayed_coding::Interval {
            start: 123,
            end: 124,
        };
        assert_eq!(
            dc_branch_new(&interval, 1, 2, &mut branch),
            DcStatus::InvalidModel
        );
        assert!(branch.is_null());
        assert_eq!(
            dc_branch_new(null(), 1, 1, &mut branch),
            DcStatus::InvalidArgument
        );
        assert_eq!(dc_branch_new(&interval, 1, 1, &mut branch), DcStatus::Ok);
        let mut workspace = null_mut();
        assert_eq!(dc_workspace_new(&mut workspace), DcStatus::Ok);
        let handles = [branch.cast_const(); 17];
        for delay in [16, 24, 32] {
            for lanes in [1, 4] {
                let mut output = [0xa5; 35];
                let (mut offset, mut size) = (99, 99);
                assert_eq!(
                    dc_encode_branches(
                        handles.as_ptr(),
                        17,
                        delay,
                        lanes,
                        output.as_mut_ptr(),
                        output.len(),
                        workspace,
                        &mut offset,
                        &mut size
                    ),
                    DcStatus::Ok
                );
                assert_eq!((offset, size), (1, 34));
                assert_eq!(output[0], 0xa5);
                assert!(output[1..]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .all(|b| *b == [0, 123]));
                output.fill(0xa5);
                assert_eq!(
                    dc_encode_branches(
                        handles.as_ptr(),
                        17,
                        delay,
                        lanes,
                        output.as_mut_ptr(),
                        1,
                        workspace,
                        &mut offset,
                        &mut size
                    ),
                    DcStatus::OutputTooSmall
                );
                assert_eq!(output, [0xa5; 35]);
                assert_eq!((offset, size), (1, 34));
                assert_eq!(
                    dc_encode_branches(
                        null(),
                        0,
                        delay,
                        lanes,
                        null_mut(),
                        0,
                        workspace,
                        &mut offset,
                        &mut size
                    ),
                    DcStatus::Ok
                );
                assert_eq!((offset, size), (0, 0));
            }
        }
        let invalid = [null()];
        let (mut offset, mut size) = (0, 0);
        assert_eq!(
            dc_encode_branches(
                invalid.as_ptr(),
                1,
                24,
                1,
                null_mut(),
                0,
                workspace,
                &mut offset,
                &mut size
            ),
            DcStatus::InvalidArgument
        );
        dc_workspace_free(workspace);
        dc_branch_free(branch);
        dc_branch_free(null_mut());
    }
}

#[test]
fn owned_handles_and_bounded_buffers() {
    unsafe {
        let mut model = null_mut();
        let mut workspace = null_mut();
        assert_eq!(dc_model_new(null(), 0, &mut model), DcStatus::InvalidModel);
        assert!(model.is_null());
        assert_eq!(
            dc_model_new(null(), 1, &mut model),
            DcStatus::InvalidArgument
        );
        assert_eq!(
            dc_model_new([65536].as_ptr(), 1, null_mut()),
            DcStatus::InvalidArgument
        );
        assert_eq!(dc_workspace_new(null_mut()), DcStatus::InvalidArgument);
        assert_eq!(dc_workspace_new(&mut workspace), DcStatus::Ok);
        for flags in 0..4 {
            assert_eq!(
                dc_model_new_with_options([1, 65535].as_ptr(), 2, flags, &mut model),
                DcStatus::Ok
            );
            let symbols = [0, 1, 1, 0, 1, 0, 1];
            for delay in [16, 24, 32] {
                for lanes in [1, 4] {
                    let mut output = [0xa5; 32];
                    let (mut offset, mut size) = (0, 0);
                    assert_eq!(
                        dc_encode_interleaved(
                            model,
                            delay,
                            lanes,
                            symbols.as_ptr(),
                            symbols.len(),
                            output.as_mut_ptr(),
                            output.len(),
                            workspace,
                            &mut offset,
                            &mut size
                        ),
                        DcStatus::Ok
                    );
                    assert!(output[..offset].iter().all(|&x| x == 0xa5));
                    let mut restored = [0; 7];
                    assert_eq!(
                        dc_decode_interleaved(
                            model,
                            delay,
                            lanes,
                            output.as_ptr().add(offset),
                            size,
                            restored.as_mut_ptr(),
                            restored.len()
                        ),
                        DcStatus::Ok
                    );
                    assert_eq!(restored, symbols);
                    if lanes == 4 {
                        assert_eq!(
                            dc_decode_lookahead_interleaved(
                                model,
                                delay,
                                lanes,
                                output.as_ptr().add(offset),
                                size,
                                restored.as_mut_ptr(),
                                restored.len()
                            ),
                            DcStatus::Ok
                        );
                        assert_eq!(restored, symbols);
                        assert_eq!(
                            dc_decode_grouped4(
                                model,
                                delay,
                                output.as_ptr().add(offset),
                                size,
                                restored.as_mut_ptr(),
                                restored.len()
                            ),
                            DcStatus::Ok
                        );
                        assert_eq!(restored, symbols);
                    }
                    if lanes == 4 {
                        restored.fill(99);
                        assert_eq!(
                            dc_decode_branchless4(
                                model,
                                delay,
                                output.as_ptr().add(offset),
                                size,
                                restored.as_mut_ptr(),
                                restored.len()
                            ),
                            DcStatus::Ok
                        );
                        assert_eq!(restored, symbols);
                    }
                    if lanes == 1 {
                        restored.fill(99);
                        assert_eq!(
                            dc_decode_lookahead(
                                model,
                                delay,
                                output.as_ptr().add(offset),
                                size,
                                restored.as_mut_ptr(),
                                restored.len()
                            ),
                            DcStatus::Ok
                        );
                        assert_eq!(restored, symbols);
                        assert_eq!(
                            dc_decode_lookahead(
                                model,
                                delay,
                                output.as_ptr().add(offset),
                                size - 1,
                                restored.as_mut_ptr(),
                                restored.len()
                            ),
                            DcStatus::TruncatedInput
                        );
                        assert_eq!(
                            dc_decode_lookahead(model, delay, null(), 0, null_mut(), 0),
                            DcStatus::Ok
                        );
                        assert_eq!(
                            dc_decode_lookahead(model, 15, null(), 0, null_mut(), 0),
                            DcStatus::InvalidDelay
                        );
                        assert_eq!(
                            dc_decode_lookahead(model, delay, null(), 1, null_mut(), 0),
                            DcStatus::InvalidArgument
                        );
                    }
                    assert_eq!(
                        dc_decode_interleaved(
                            model,
                            delay,
                            lanes,
                            output.as_ptr().add(offset),
                            size - 1,
                            restored.as_mut_ptr(),
                            restored.len()
                        ),
                        DcStatus::TruncatedInput
                    );
                    assert_eq!(
                        dc_encode_interleaved(
                            model,
                            delay,
                            lanes,
                            symbols.as_ptr(),
                            symbols.len(),
                            null_mut(),
                            0,
                            workspace,
                            &mut offset,
                            &mut size
                        ),
                        DcStatus::OutputTooSmall
                    );
                    assert_eq!(
                        dc_decode_interleaved(model, delay, lanes, null(), 0, null_mut(), 0),
                        DcStatus::Ok
                    );
                }
            }
            dc_model_free(model);
        }
        dc_workspace_free(workspace);
        dc_workspace_free(null_mut());
        dc_model_free(null_mut());
    }
}
