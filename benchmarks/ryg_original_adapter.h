// SPDX-License-Identifier: MIT
// Complete loops are mechanically extracted from pinned upstream .cpp files.
#pragma once
#include "upstream_loops.generated.h"

// Kind: byte (0), rans64 (1), upstream eight-state SSE4.1 demo (2).
// Input/output width is independent of codec; calibration tests both u8 and u32.
template<unsigned Kind, unsigned Precision = Kind == 2 ? 12 : 16>
struct OriginalRans {
    original_rans::SymbolStats stats{};
    std::vector<uint8_t> lookup, byte_storage;
    std::vector<uint32_t> word_storage;
    std::array<RansEncSymbol, 256> byte_enc{};
    std::array<RansDecSymbol, 256> byte_dec{};
    std::array<Rans64EncSymbol, 256> word_enc{};
    std::array<Rans64DecSymbol, 256> word_dec{};
#ifdef DELAYED_CODING_HAVE_RANS_SIMD
    RansWordTables tables{};
    std::vector<uint16_t> simd_storage;
#endif
    size_t offset = 0, size = 0;
    static constexpr uint32_t bits = Precision;
    OriginalRans(const Model& model, size_t n)
        : lookup(size_t(1) << bits), byte_storage(n * 2 + 8), word_storage(n + 4)
#ifdef DELAYED_CODING_HAVE_RANS_SIMD
          , simd_storage(n + 16 + 4)
#endif
          {
        require(bits >= 8 && bits <= 16);
        if constexpr (Kind == 2) require(bits == 12);
        uint32_t start = 0;
        for (unsigned symbol = 0; symbol < 256; ++symbol) {
            const auto scale = 65536 >> bits;
            require(model.frequencies[symbol] % scale == 0);
            const auto frequency = model.frequencies[symbol] / scale;
            stats.freqs[symbol] = frequency;
            stats.cum_freqs[symbol] = start;
            std::fill(lookup.begin() + start, lookup.begin() + start + frequency, uint8_t(symbol));
            if constexpr (Kind == 0) {
                require(frequency < 65536); // upstream u16 decoder symbol cannot hold 65536
                RansEncSymbolInit(&byte_enc[symbol], start, frequency, bits);
                RansDecSymbolInit(&byte_dec[symbol], start, frequency);
            } else if constexpr (Kind == 1) {
                Rans64EncSymbolInit(&word_enc[symbol], start, frequency, bits);
                Rans64DecSymbolInit(&word_dec[symbol], start, frequency);
            }
#ifdef DELAYED_CODING_HAVE_RANS_SIMD
            else {
                require(frequency < RANS_WORD_M);
                RansWordTablesInitSymbol(&tables, symbol, start, frequency);
            }
#endif
            start += frequency;
        }
        stats.cum_freqs[256] = start;
        require(start == uint32_t(1) << bits);
    }

    template<class Input> void encode(const std::vector<Input>& input) {
        if constexpr (Kind == 0) {
            auto* begin = original_rans::encode_byte_2(input.data(), input.size(), byte_storage.data(), byte_storage.size(), byte_enc.data());
            offset = begin - byte_storage.data(); size = byte_storage.size() - offset;
        } else if constexpr (Kind == 1) {
            auto* end = word_storage.data() + word_storage.size();
            auto* begin = original_rans::encode64_2<Input,bits>(input.data(), input.size(), end, word_enc.data());
            offset = begin - word_storage.data(); size = (end - begin) * 4;
        }
#ifdef DELAYED_CODING_HAVE_RANS_SIMD
        else {
            auto* end = simd_storage.data() + simd_storage.size() - 4;
            auto* begin = original_rans::encode_simd_8(input.data(), input.size(), end, stats);
            offset = begin - simd_storage.data(); size = (end - begin) * 2;
        }
#endif
    }

    template<class Output> void decode(std::vector<Output>& output) {
        if constexpr (Kind == 0)
            original_rans::decode_byte_2<Output,bits>(byte_storage.data() + offset, output.data(), output.size(), lookup.data(), byte_dec.data(), byte_storage.data() + byte_storage.size());
        else if constexpr (Kind == 1)
            original_rans::decode64_2<Output,bits>(word_storage.data() + offset, output.data(), output.size(), lookup.data(), word_dec.data(), word_storage.data() + word_storage.size());
#ifdef DELAYED_CODING_HAVE_RANS_SIMD
        else
            original_rans::decode_simd_8(simd_storage.data() + offset, output.data(), output.size(), tables, simd_storage.data() + simd_storage.size() - 4);
#endif
    }
    size_t bytes() const { return size; }
};
