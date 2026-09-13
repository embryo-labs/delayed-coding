/* SPDX-License-Identifier: MIT */
#ifndef DELAYED_CODING_H
#define DELAYED_CODING_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif

typedef struct DcModel DcModel;
typedef struct DcWorkspace DcWorkspace;
typedef enum DcStatus {
    DC_OK = 0, DC_INVALID_ARGUMENT = 1, DC_INVALID_MODEL = 2,
    DC_INVALID_SYMBOL = 3, DC_INVALID_DELAY = 4, DC_INPUT_TOO_LARGE = 5,
    DC_OUTPUT_TOO_SMALL = 6, DC_TRUNCATED_INPUT = 7, DC_TRAILING_INPUT = 8,
    DC_INVALID_STATE = 9, DC_PANIC = 10
} DcStatus;

/* Experimental ABI (0.x). Frequencies sum to 65536, alphabet size 1..65536.
 * Models can be shared by threads; workspaces require exclusive access.
 * Buffers and output metadata must be aligned, valid and nonoverlapping.
 * A null buffer is permitted only with length zero. Free each handle exactly once.
 * Payloads are big-endian 16-bit words, with no framing or model serialization.
 * The caller must preserve the same model, symbol count and delay (16, 24 or 32).
 * Rust panics are caught and returned as DC_PANIC; allocator exhaustion can abort.
 */
DcStatus dc_model_new(const uint32_t* frequencies, size_t count, DcModel** out);
/* flags: 1 = 128 KiB direct encode table; 2 = 512 KiB direct decode table.
 * Combine flags with bitwise OR. These options do not change the payload format. */
DcStatus dc_model_new_with_options(const uint32_t* frequencies, size_t count, uint32_t flags, DcModel** out);
void dc_model_free(DcModel* model);
DcStatus dc_workspace_new(DcWorkspace** out);
void dc_workspace_free(DcWorkspace* workspace);

/* Output is written backwards. On success payload = output + offset, size bytes.
 * Capacity 2*count is always sufficient when count does not overflow size_t.
 * Invalid symbols/capacity are rejected before writing payload bytes.
 * Offset and size are only assigned on success. Workspace can grow internally.
 */
DcStatus dc_encode(const DcModel* model, uint32_t delay,
    const uint32_t* symbols, size_t count, uint8_t* output, size_t capacity,
    DcWorkspace* workspace, size_t* offset, size_t* size);

/* Decodes exactly count symbols. Output can contain a prefix on error.
 * Input consumption/final-state checks do not replace an external checksum.
 */
DcStatus dc_decode(const DcModel* model, uint32_t delay,
    const uint8_t* input, size_t size, uint32_t* output, size_t count);

/* Experimental single-lane fixed-model physical-word lookahead. Same format and
 * checks as dc_decode. No extra table/allocation. Workload-dependent speed. */
DcStatus dc_decode_lookahead(const DcModel* model, uint32_t delay,
    const uint8_t* input, size_t size, uint32_t* output, size_t count);

/* Same contracts as above, with 1 or 4 round-robin coding states.
 * Lane count must be stored externally with delay/model/symbol count. */
DcStatus dc_encode_interleaved(const DcModel* model, uint32_t delay, uint32_t lanes,
    const uint32_t* symbols, size_t count, uint8_t* output, size_t capacity,
    DcWorkspace* workspace, size_t* offset, size_t* size);
DcStatus dc_decode_interleaved(const DcModel* model, uint32_t delay, uint32_t lanes,
    const uint8_t* input, size_t size, uint32_t* output, size_t count);
#ifdef __cplusplus
}
#endif
#endif
