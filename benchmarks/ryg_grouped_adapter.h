// SPDX-License-Identifier: MIT
// Explicitly DERIVED control, not an original upstream four-state demo.
// Extend main64.cpp's lookup-all / step-all / renorm-all ordering to four lanes.
#pragma once
#include <utility>

template<unsigned Lanes>
struct Rans64Grouped : Rans<Lanes, true> {
    using Base = Rans<Lanes, true>;
    std::array<Rans64DecSymbol, 256> decode_symbols{};
    Rans64Grouped(const Model& model, size_t n) : Base(model, n) {
        for (unsigned i=0; i<256; ++i)
            Rans64DecSymbolInit(&decode_symbols[i], model.starts[i], model.frequencies[i]);
    }
    template<size_t... I>
    void encode_group(std::array<Rans64State,Lanes>& states, uint32_t*& cursor,
                      const std::vector<uint32_t>& input, size_t base, std::index_sequence<I...>) {
        (Rans64EncPutSymbol(&states[Lanes-1-I], &cursor,
            &this->model.word_symbols[input[base+Lanes-1-I]], 16), ...);
    }
    void encode(const std::vector<uint32_t>& input) {
        std::array<Rans64State,Lanes> states;
        for (auto& state : states) Rans64EncInit(&state);
        auto* end = this->words_storage.data() + this->words_storage.size();
        auto* cursor = end;
        size_t i = input.size();
        while (i % Lanes) {
            --i;
            Rans64EncPutSymbol(&states[i % Lanes], &cursor, &this->model.word_symbols[input[i]], 16);
        }
        while (i != 0) {
            i -= Lanes;
            encode_group(states, cursor, input, i, std::make_index_sequence<Lanes>{});
        }
        for (size_t lane=Lanes; lane-- != 0;) Rans64EncFlush(&states[lane], &cursor);
        this->offset = cursor - this->words_storage.data(); this->size = (end-cursor)*4;
    }
    template<size_t... I>
    void decode_group(std::array<Rans64State,Lanes>& states, uint32_t*& cursor,
                      uint32_t* output, std::index_sequence<I...>) {
        std::array<uint32_t,Lanes> symbols;
        ((symbols[I] = this->model.lookup[Rans64DecGet(&states[I], 16)]), ...);
        ((output[I] = symbols[I]), ...);
        (Rans64DecAdvanceSymbolStep(&states[I], &decode_symbols[symbols[I]], 16), ...);
        (Rans64DecRenorm(&states[I], &cursor), ...);
    }
    void decode(std::vector<uint32_t>& output) {
        std::array<Rans64State,Lanes> states;
        auto* cursor = this->words_storage.data() + this->offset;
        for (auto& state : states) Rans64DecInit(&state, &cursor);
        size_t i=0;
        for (; i+Lanes <= output.size(); i+=Lanes)
            decode_group(states, cursor, output.data()+i, std::make_index_sequence<Lanes>{});
        for (; i<output.size(); ++i) {
            auto& state = states[i % Lanes];
            const auto symbol = this->model.lookup[Rans64DecGet(&state, 16)];
            output[i] = symbol;
            Rans64DecAdvanceSymbol(&state, &cursor, &decode_symbols[symbol], 16);
        }
        require(cursor == this->words_storage.data() + this->words_storage.size());
        for (auto state : states) require(state == RANS64_L);
    }
    void validate_with_generic(const std::vector<uint32_t>& input) {
        Base reference(this->model, input.size());
        reference.encode(input); encode(input);
        require(reference.bytes() == this->bytes());
        require(std::equal(reference.words_storage.begin()+reference.offset,
            reference.words_storage.end(), this->words_storage.begin()+this->offset));
    }
};
