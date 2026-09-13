# Mechanical extraction of upstream's complete hot loops. No source checkout is
# edited. Hashes make changes of upstream version/rewrite boundaries fail closed.
foreach(pair
    "rans_byte.h|66e9521b4485c1b36ee7527f1a6cd7c9fe7f8495f81420602fbdc26b7273bea6"
    "rans64.h|aef84257cf7a467e7ef134beddf743a750e218b7231f23c1a02ca1ea00683717"
    "rans_word_sse41.h|0b00a7a63f243412bf2cb62ba03d2aae63c6a14231cd9455bca040150d5ab73e"
    "platform.h|cf8020273bfde03d6e850e7a74a162b4657825a4b7c98606e309a1586c328d1c"
    "main.cpp|eb8e727c1dd9741a5484c5b4e4cadcb0baa048b2ddd46421cccf750f025a9bc3"
    "main64.cpp|92a869c703ca6f6738c37c80bdab9eb05d613755f1b06dd7b6195264d99c32a3"
    "main_simd.cpp|56a62763f98dba0bc4d8b633e5cd6e7d4839247e656a56a5213b44a13b989f7b")
    string(REPLACE "|" ";" parts "${pair}")
    list(GET parts 0 file)
    list(GET parts 1 expected)
    file(SHA256 "${UPSTREAM}/${file}" actual)
    if(NOT actual STREQUAL expected)
        message(FATAL_ERROR "${file}: expected pinned ryg_rans c9d162d; do not silently change calibration loops")
    endif()
endforeach()

function(extract file section begin end result)
    file(READ "${UPSTREAM}/${file}" source)
    string(FIND "${source}" "${section}" index)
    if(index LESS 0)
        message(FATAL_ERROR "Missing upstream section ${section}")
    endif()
    string(SUBSTRING "${source}" ${index} -1 source)
    string(FIND "${source}" "${begin}" first)
    string(FIND "${source}" "${end}" last)
    if(first LESS 0 OR last LESS first)
        message(FATAL_ERROR "Invalid upstream extraction ${file}: ${begin}")
    endif()
    math(EXPR length "${last} - ${first}")
    string(SUBSTRING "${source}" ${first} ${length} body)
    set(${result} "${body}" PARENT_SCOPE)
endfunction()

extract(main64.cpp "struct SymbolStats" "struct SymbolStats" "\nint main()" stats)
extract(main64.cpp "// try interleaved rANS encode" "Rans64State rans0, rans1;" "uint64_t enc_clocks" encode64)
extract(main64.cpp "// try interleaved rANS decode" "Rans64State rans0, rans1;" "uint64_t dec_clocks" decode64)
extract(main.cpp "// try interleaved rANS encode" "RansState rans0, rans1;" "uint64_t enc_clocks" encode_byte)
extract(main.cpp "// try interleaved rANS decode" "RansState rans0, rans1;" "uint64_t dec_clocks" decode_byte)
extract(main_simd.cpp "// try SIMD rANS encode" "RansWordEnc rans[8];" "uint64_t enc_clocks" encode_simd)
extract(main_simd.cpp "// try SIMD rANS decode" "RansSimdDec rans0, rans1;" "uint64_t dec_clocks" decode_simd)
# Only the two packed output stores change: portable byte memcpy or widening to
# the uniform benchmark's u32 representation. Source/lookup/update/renorm order
# and upstream tail handling are preserved verbatim.
string(REPLACE "*(uint32_t *)(dec_bytes + i) = s03;" "store_four(dec_bytes + i, s03);" decode_simd "${decode_simd}")
string(REPLACE "*(uint32_t *)(dec_bytes + i + 4) = s47;" "store_four(dec_bytes + i + 4, s47);" decode_simd "${decode_simd}")
file(WRITE "${OUTPUT}" "// Generated from hash-checked public-domain ryg_rans demos. Do not edit.\n#pragma once\n#include <cassert>\n#include <cstring>\n#include <type_traits>\nnamespace original_rans {\n${stats}\n")
file(APPEND "${OUTPUT}" "
template<class Input, uint32_t ProbBits> uint32_t* encode64_2(const Input* in_bytes, size_t in_size, uint32_t* out_end, const Rans64EncSymbol* esyms) {
    static constexpr uint32_t prob_bits = ProbBits;
    uint32_t* rans_begin;
    ${encode64}
    return rans_begin;
}
template<class Output, uint32_t ProbBits> void decode64_2(uint32_t* rans_begin, Output* dec_bytes, size_t in_size, const uint8_t* cum2sym, const Rans64DecSymbol* dsyms, const uint32_t* end) {
    static constexpr uint32_t prob_bits = ProbBits;
    ${decode64}
    if (ptr != end || rans0 != RANS64_L || rans1 != RANS64_L) throw std::runtime_error(\"upstream rans64 final state\");
}
template<class Input> uint8_t* encode_byte_2(const Input* in_bytes, size_t in_size, uint8_t* out_buf, size_t out_max_size, const RansEncSymbol* esyms) {
    uint8_t* rans_begin;
    ${encode_byte}
    return rans_begin;
}
template<class Output, uint32_t ProbBits> void decode_byte_2(uint8_t* rans_begin, Output* dec_bytes, size_t in_size, const uint8_t* cum2sym, const RansDecSymbol* dsyms, const uint8_t* end) {
    static constexpr uint32_t prob_bits = ProbBits;
    ${decode_byte}
    if (ptr != end || rans0 != RANS_BYTE_L || rans1 != RANS_BYTE_L) throw std::runtime_error(\"upstream byte-rans final state\");
}
#ifdef DELAYED_CODING_HAVE_RANS_SIMD
template<class Output> inline void store_four(Output* out, uint32_t packed) {
    if constexpr (std::is_same_v<Output, uint8_t>) std::memcpy(out, &packed, 4);
    else _mm_storeu_si128(reinterpret_cast<__m128i*>(out), _mm_cvtepu8_epi32(_mm_cvtsi32_si128(packed)));
}
template<class Input> uint16_t* encode_simd_8(const Input* in_bytes, size_t in_size, uint16_t* out_end, const SymbolStats& stats) {
    uint8_t* out_buf = reinterpret_cast<uint8_t*>(out_end);
    const size_t out_max_size = 0;
    uint16_t* rans_begin;
    ${encode_simd}
    return rans_begin;
}
template<class Output> void decode_simd_8(uint16_t* rans_begin, Output* dec_bytes, size_t in_size, const RansWordTables& tab, const uint16_t* end) {
    ${decode_simd}
    if (ptr != end) throw std::runtime_error(\"upstream SIMD input consumption\");
    for (unsigned i=0; i<4; ++i) if (rans0.lane[i] != RANS_WORD_L || rans1.lane[i] != RANS_WORD_L) throw std::runtime_error(\"upstream SIMD final state\");
}
#endif
} // namespace original_rans
")
