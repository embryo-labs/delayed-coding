// SPDX-License-Identifier: MIT
// Uses unmodified external ryg_rans headers. No dependency is downloaded by this build.
#include "delayed_coding.h"
#include "rans_byte.h"
#include "rans64.h"
#include <algorithm>
#include <array>
#include <chrono>
#include <cstdint>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <random>
#include <stdexcept>
#include <string>
#include <vector>

using Clock = std::chrono::steady_clock;
static volatile uint64_t checksum = 0;
static bool validate_only = false;
#ifdef DELAYED_CODING_HAVE_RANS_SIMD
static constexpr uint32_t probability_scale = 4096;
#else
static constexpr uint32_t probability_scale = 65536;
#endif
static void require(bool condition) { if (!condition) throw std::runtime_error("benchmark validation failed"); }

struct Model {
    std::array<uint32_t, 256> frequencies{}, starts{};
    std::array<uint8_t, 65536> lookup{};
    // Equal 512 KiB packed decode-table option for rANS and DC comparisons.
    std::vector<uint64_t> packed = std::vector<uint64_t>(65536);
    std::array<RansEncSymbol, 256> byte_symbols{};
    std::array<Rans64EncSymbol, 256> word_symbols{};
    explicit Model(std::array<uint32_t, 256> weights) : frequencies(weights) {
        uint32_t position = 0;
        for (unsigned i = 0; i < 256; ++i) {
            starts[i] = position;
            for (unsigned j = 0; j < frequencies[i]; ++j) {
                packed[position] = (uint64_t(i) << 32) | (uint64_t(frequencies[i] - 1) << 16) | j;
                lookup[position++] = i;
            }
            RansEncSymbolInit(&byte_symbols[i], starts[i], frequencies[i], 16);
            Rans64EncSymbolInit(&word_symbols[i], starts[i], frequencies[i], 16);
        }
        require(position == 65536);
    }
};

#ifdef DELAYED_CODING_HAVE_RANS_ALIAS
#include "ryg_alias_adapter.h"
#endif
#ifdef DELAYED_CODING_HAVE_RANS_SIMD
#include "ryg_simd_adapter.h"
#endif

struct Delayed {
    DcModel* model = nullptr;
    DcWorkspace* workspace = nullptr;
    std::vector<uint8_t> buffer;
    size_t offset = 0, size = 0;
    uint32_t delay;
    uint32_t lanes;
    bool lookahead = false;
    bool grouped = false;
    Delayed(const Model& m, size_t n, uint32_t d, uint32_t flags = 0, uint32_t l = 1)
        : buffer(n * 2), delay(d), lanes(l) {
        require(dc_model_new_with_options(m.frequencies.data(), 256, flags, &model) == DC_OK);
        require(dc_workspace_new(&workspace) == DC_OK);
    }
    ~Delayed() { dc_model_free(model); dc_workspace_free(workspace); }
    void encode(const std::vector<uint32_t>& input) {
        require(dc_encode_interleaved(model, delay, lanes, input.data(), input.size(), buffer.data(), buffer.size(),
                          workspace, &offset, &size) == DC_OK);
    }
    void decode(std::vector<uint32_t>& output) {
        if (grouped) {
            require(dc_decode_grouped4(model, delay, buffer.data() + offset, size, output.data(), output.size()) == DC_OK);
            return;
        }
        if (lookahead) {
            require(dc_decode_lookahead_interleaved(model, delay, lanes, buffer.data() + offset, size, output.data(), output.size()) == DC_OK);
            return;
        }
        require(dc_decode_interleaved(model, delay, lanes, buffer.data() + offset, size, output.data(), output.size()) == DC_OK);
    }
    size_t bytes() const { return size; }
};

template<unsigned Lanes, bool Word, bool Packed = false>
struct Rans {
    const Model& model;
    std::vector<uint8_t> bytes_storage;
    std::vector<uint32_t> words_storage;
    size_t offset = 0, size = 0;
    Rans(const Model& m, size_t n) : model(m), bytes_storage(n * 2 + 4 * Lanes),
                                                words_storage(n + 2 * Lanes) {}
    void encode(const std::vector<uint32_t>& input) {
        if constexpr (Word) {
            std::array<Rans64State, Lanes> states;
            for (auto& state : states) Rans64EncInit(&state);
            auto* end = words_storage.data() + words_storage.size();
            auto* cursor = end;
            for (size_t i = input.size(); i-- != 0;)
                Rans64EncPutSymbol(&states[i % Lanes], &cursor, &model.word_symbols[input[i]], 16);
            for (size_t lane = Lanes; lane-- != 0;) Rans64EncFlush(&states[lane], &cursor);
            offset = cursor - words_storage.data(); size = (end - cursor) * 4;
        } else {
            std::array<RansState, Lanes> states;
            for (auto& state : states) RansEncInit(&state);
            auto* end = bytes_storage.data() + bytes_storage.size();
            auto* cursor = end;
            for (size_t i = input.size(); i-- != 0;)
                RansEncPutSymbol(&states[i % Lanes], &cursor, &model.byte_symbols[input[i]]);
            for (size_t lane = Lanes; lane-- != 0;) RansEncFlush(&states[lane], &cursor);
            offset = cursor - bytes_storage.data(); size = end - cursor;
        }
    }
    void decode(std::vector<uint32_t>& output) {
        if constexpr (Word) {
            auto* cursor = words_storage.data() + offset;
            std::array<Rans64State, Lanes> states;
            for (auto& state : states) Rans64DecInit(&state, &cursor);
            for (size_t i = 0; i < output.size(); ++i) {
                auto& state = states[i % Lanes];
                if constexpr (Packed) {
                    const auto entry = model.packed[Rans64DecGet(&state, 16)];
                    output[i] = entry >> 32;
                    state = (state >> 16) * (uint32_t((entry >> 16) & 65535) + 1) + (entry & 65535);
                    Rans64DecRenorm(&state, &cursor);
                } else {
                    const auto symbol = model.lookup[Rans64DecGet(&state, 16)];
                    output[i] = symbol;
                    Rans64DecAdvance(&state, &cursor, model.starts[symbol], model.frequencies[symbol], 16);
                }
            }
            require(cursor == words_storage.data() + words_storage.size());
            for (auto state : states) require(state == RANS64_L);
        } else {
            auto* cursor = bytes_storage.data() + offset;
            std::array<RansState, Lanes> states;
            for (auto& state : states) RansDecInit(&state, &cursor);
            for (size_t i = 0; i < output.size(); ++i) {
                auto& state = states[i % Lanes];
                if constexpr (Packed) {
                    const auto entry = model.packed[RansDecGet(&state, 16)];
                    output[i] = entry >> 32;
                    state = (state >> 16) * (uint32_t((entry >> 16) & 65535) + 1) + (entry & 65535);
                    RansDecRenorm(&state, &cursor);
                } else {
                    const auto symbol = model.lookup[RansDecGet(&state, 16)];
                    output[i] = symbol;
                    RansDecAdvance(&state, &cursor, model.starts[symbol], model.frequencies[symbol], 16);
                }
            }
            require(cursor == bytes_storage.data() + bytes_storage.size());
            for (auto state : states) require(state == RANS_BYTE_L);
        }
    }
    size_t bytes() const { return size; }
};

template<class Function>
double median_ns(size_t repeats, Function run) {
    std::vector<double> samples;
    for (int sample = 0; sample < 7; ++sample) {
        const auto begin = Clock::now();
        for (size_t i = 0; i < repeats; ++i) run();
        samples.push_back(std::chrono::duration<double, std::nano>(Clock::now() - begin).count() / repeats);
    }
    std::sort(samples.begin(), samples.end());
    return samples[samples.size() / 2];
}

template<class Codec>
void measure(const std::string& distribution, const std::string& name, Codec& codec,
             const std::vector<uint32_t>& input) {
    std::vector<uint32_t> output(input.size());
    codec.encode(input); codec.decode(output); require(output == input);
    if (validate_only) return;
    const auto bytes = codec.bytes();
    const size_t repeats = std::max<size_t>(1, 262144 / input.size());
    const double encode_ns = median_ns(repeats, [&] { codec.encode(input); checksum += codec.bytes(); });
    const double decode_ns = median_ns(repeats, [&] { codec.decode(output); checksum += output[input.size() / 2]; });
    require(output == input);
    std::cout << distribution << ',' << input.size() << ',' << name << ',' << bytes << ','
              << (bytes * 8.0 / input.size()) << ',' << encode_ns / input.size() << ','
              << decode_ns / input.size() << ',' << encode_ns << ',' << decode_ns << '\n';
}

static void benchmark_input(const std::string &distribution, const Model &model,
                            const std::vector<uint32_t> &input) {
    const size_t count = input.size();
    Delayed d16(model, count, 16), d24(model, count, 24), d32(model, count, 32);
    Delayed de(model, count, 24, 1), dd(model, count, 24, 2), db(model, count, 24, 3);
    Delayed d4(model, count, 24, 0, 4), d4e(model, count, 24, 1, 4), d4b(model, count, 24, 3, 4);
    Delayed ahead(model, count, 24), ahead_direct(model, count, 24, 2);
    ahead.lookahead = ahead_direct.lookahead = true;
    Delayed ahead4(model, count, 24, 0, 4), ahead4_direct(model, count, 24, 2, 4);
    ahead4.lookahead = ahead4_direct.lookahead = true;
    Delayed grouped4(model, count, 24, 0, 4), grouped4_direct(model, count, 24, 2, 4);
    grouped4.grouped = grouped4_direct.grouped = true;
    Rans<1, false> b1(model, count);
    Rans<4, false> b4(model, count);
    Rans<1, true> w1(model, count);
    Rans<4, true> w4(model, count);
    Rans<1, true, true> wp1(model, count);
    Rans<4, true, true> wp4(model, count);
    Rans<1, false, true> bp1(model, count);
    Rans<4, false, true> bp4(model, count);
#ifdef DELAYED_CODING_HAVE_RANS_ALIAS
    RansAlias<1> a1(model, count);
    RansAlias<4> a4(model, count);
#endif
    measure(distribution, "delayed16", d16, input);
    measure(distribution, "delayed24", d24, input);
    measure(distribution, "delayed32", d32, input);
    measure(distribution, "delayed24_direct_encode", de, input);
    measure(distribution, "delayed24_direct_decode", dd, input);
    measure(distribution, "delayed24_direct_both", db, input);
    measure(distribution, "delayed24_4", d4, input);
    measure(distribution, "delayed24_4_direct_encode", d4e, input);
    measure(distribution, "delayed24_4_direct_both", d4b, input);
    measure(distribution, "delayed24_lookahead", ahead, input);
    measure(distribution, "delayed24_lookahead_direct", ahead_direct, input);
    measure(distribution, "delayed24_4_lookahead", ahead4, input);
    measure(distribution, "delayed24_4_lookahead_direct", ahead4_direct, input);
    measure(distribution, "delayed24_4_grouped", grouped4, input);
    measure(distribution, "delayed24_4_grouped_direct", grouped4_direct, input);
    measure(distribution, "rans_byte_1", b1, input);
    measure(distribution, "rans_byte_4", b4, input);
    measure(distribution, "rans64_1", w1, input);
    measure(distribution, "rans64_4", w4, input);
    measure(distribution, "rans64_packed_1", wp1, input);
    measure(distribution, "rans64_packed_4", wp4, input);
    measure(distribution, "rans_byte_packed_1", bp1, input);
    measure(distribution, "rans_byte_packed_4", bp4, input);
#ifdef DELAYED_CODING_HAVE_RANS_ALIAS
    measure(distribution, "rans_alias_1", a1, input);
    measure(distribution, "rans_alias_4", a4, input);
#endif
#ifdef DELAYED_CODING_HAVE_RANS_SIMD
    if (*std::max_element(model.frequencies.begin(), model.frequencies.end()) < 65536) {
        RansSimd simd(model, count);
        measure(distribution, "rans_sse41_4", simd, input);
    } else {
        std::cerr << "upstream SIMD variant skipped: one-symbol model is unsupported\n";
    }
#endif
}

// A deterministic shared normalization policy, performed once outside timing.
// Reserve one slot per observed byte, then apportion by largest remainder.
static std::array<uint32_t, 256> normalize_counts(const std::vector<uint32_t> &input) {
    std::array<uint32_t, 256> counts{}, weights{};
    for (auto symbol : input)
        ++counts[symbol];
    const uint32_t active = std::count_if(counts.begin(), counts.end(), [](auto n) { return n != 0; });
    const uint64_t remaining = probability_scale - active;
    std::vector<std::pair<uint64_t, unsigned>> remainders;
    uint32_t assigned = 0;
    for (unsigned symbol = 0; symbol < 256; ++symbol) {
        if (counts[symbol] == 0) continue;
        const uint64_t scaled = counts[symbol] * remaining;
        weights[symbol] = 1 + scaled / input.size();
        assigned += weights[symbol];
        remainders.emplace_back(scaled % input.size(), symbol);
    }
    std::sort(remainders.begin(), remainders.end(), [](auto a, auto b) {
        return a.first != b.first ? a.first > b.first : a.second < b.second;
    });
    for (size_t i = 0; i < probability_scale - assigned; ++i) ++weights[remainders.at(i).second];
    for (auto& weight : weights) weight *= 65536 / probability_scale;
    return weights;
}

static std::vector<uint32_t> read_symbols(const std::string& path) {
    std::ifstream file(path, std::ios::binary | std::ios::ate);
    if (!file) throw std::invalid_argument("cannot open input file");
    const auto length = file.tellg();
    if (length <= 0 || length > (1u << 26)) throw std::invalid_argument("file must contain 1..67108864 bytes");
    std::vector<unsigned char> bytes(static_cast<size_t>(length));
    file.seekg(0);
    if (!file.read(reinterpret_cast<char*>(bytes.data()), bytes.size()))
        throw std::runtime_error("cannot read complete input file");
    std::cerr << "input file=" << path << " bytes=" << bytes.size() << '\n';
    return {bytes.begin(), bytes.end()};
}

template<class Codec>
static void record_sizes(const char* name, Codec& codec, const std::vector<uint32_t>& input, size_t width) {
    uint64_t payload = 0, records = 0;
    std::vector<uint32_t> block, decoded;
    block.reserve(width); decoded.reserve(width);
    for (size_t begin = 0; begin < input.size(); begin += width) {
        const auto end = std::min(input.size(), begin + width);
        block.assign(input.begin() + begin, input.begin() + end);
        decoded.resize(block.size());
        codec.encode(block); codec.decode(decoded); require(block == decoded);
        payload += codec.bytes(); ++records;
    }
    // Same u32 offset index for every codec; symbol count/width and model external.
    const uint64_t index = 4 * (records + 1);
    if (!validate_only) std::cout << width << ',' << records << ',' << name << ',' << payload << ','
        << index << ',' << payload + index << ',' << double(payload) / records << '\n';
}

int main(int argc, char** argv) {
    try {
#ifdef DELAYED_CODING_HAVE_RANS_SIMD
        if (!__builtin_cpu_supports("sse4.1")) throw std::runtime_error("SSE4.1 CPU required");
        std::cerr << "12-bit probability suite: all weights scaled exactly to 16 bits for non-SIMD codecs\n";
#endif
        if (argc >= 3 && std::string(argv[1]) == "--records") {
            if (argc > 4 || (argc == 4 && std::string(argv[3]) != "--check"))
                throw std::invalid_argument("usage: compare_rans --records PATH [--check]");
            validate_only = argc == 4;
            const auto input = read_symbols(argv[2]);
            const Model model(normalize_counts(input));
            std::cout << "record_symbols,records,codec,payload_bytes,index_bytes,total_bytes,mean_payload_bytes\n";
            for (size_t width : {8, 16, 32, 64, 256, 4096}) {
                Delayed d16(model, width, 16), d24(model, width, 24);
                Delayed d16x4(model, width, 16, 0, 4), d24x4(model, width, 24, 0, 4);
                Rans<1, false> byte(model, width);
                Rans<1, true> word(model, width);
                Rans<4, false> byte4(model, width);
                Rans<4, true> word4(model, width);
                record_sizes("delayed16", d16, input, width);
                record_sizes("delayed24", d24, input, width);
                record_sizes("rans_byte_1", byte, input, width);
                record_sizes("rans64_1", word, input, width);
                record_sizes("delayed16_4", d16x4, input, width);
                record_sizes("delayed24_4", d24x4, input, width);
                record_sizes("rans_byte_4", byte4, input, width);
                record_sizes("rans64_4", word4, input, width);
            }
            return 0;
        }
        const bool from_file = argc > 1 && std::string(argv[1]) == "--file";
        const int base_args = from_file ? 3 : 2;
        if ((from_file && argc < 3) || argc > base_args + 1 ||
            (argc == base_args + 1 && std::string(argv[base_args]) != "--check"))
            throw std::invalid_argument("usage: compare_rans [symbols | --file PATH | --records PATH] [--check]");
        validate_only = argc == base_args + 1;
        std::cout << "distribution,symbols,codec,payload_bytes,bits_per_symbol,encode_ns_per_symbol,decode_ns_per_symbol,encode_ns_per_block,decode_ns_per_block\n";
        std::cout << std::fixed << std::setprecision(4);
        if (from_file) {
            const auto input = read_symbols(argv[2]);
            const Model model(normalize_counts(input));
            benchmark_input(probability_scale == 4096 ? "file_p12" : "file", model, input);
        } else {
            const size_t count = argc > 1 ? std::stoull(argv[1]) : 4096;
            if (count == 0 || count > (1u << 26)) throw std::invalid_argument("symbols must be 1..67108864");
            std::mt19937 random(123456);
            for (const std::string distribution : {"uniform256", "uniform16", "ramp256", "skewed", "near_constant"}) {
                std::array<uint32_t, 256> weights{};
                if (distribution == "uniform256") weights.fill(probability_scale / 256);
                if (distribution == "uniform16") for (size_t i = 0; i < 16; ++i) weights[i] = probability_scale / 16;
                if (distribution == "skewed") { weights.fill(probability_scale / 512); weights[0] += probability_scale / 2; }
                if (distribution == "near_constant") { weights.fill(1); weights[0] = probability_scale - 255; }
                // Nonuniform high-entropy distribution, exactly normalized in each suite.
                if (distribution == "ramp256") for (size_t i = 0; i < 256; ++i)
                    weights[i] = probability_scale == 65536 ? 2 * i + 1 : i / 8 + 1;
                if (distribution == "ramp256" && probability_scale == 4096)
                    for (size_t i = 128; i < 256; ++i) --weights[i];
                for (auto& weight : weights) weight *= 65536 / probability_scale;
                const Model model(weights);
                std::vector<uint32_t> input(count);
                for (auto& symbol : input) symbol = model.lookup[random() & 65535];
                benchmark_input(distribution + (probability_scale == 4096 ? "_p12" : ""), model, input);
            }
        }
        std::cerr << "validation checksum=" << checksum << '\n';
    } catch (const std::exception& error) { std::cerr << error.what() << '\n'; return 1; }
}
