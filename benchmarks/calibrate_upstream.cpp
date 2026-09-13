// Calibration only: reuse the shared models/timers, not its main driver.
#define DC_CALIBRATION_DRIVER
#include <cassert> // original demos enable assertions before including rANS
#include "compare_rans.cpp"

template<unsigned Kind, unsigned Precision, class Symbol>
static void validate_reference(OriginalRans<Kind,Precision>& codec, const std::vector<Symbol>& input) {
    codec.encode(input);
    // Independent scalar-per-symbol encoder: same lane assignment/precision.
    // Exact bytes ensure extraction and width adaptation did not change coding.
    if constexpr (Kind == 1) {
        std::vector<uint32_t> storage(input.size() + 4);
        auto* end = storage.data() + storage.size(); auto* cursor = end;
        std::array<Rans64State, 2> states;
        for (auto& state : states) Rans64EncInit(&state);
        for (size_t i = input.size(); i-- != 0;)
            Rans64EncPutSymbol(&states[i % 2], &cursor, &codec.word_enc[input[i]], codec.bits);
        for (size_t i = 2; i-- != 0;) Rans64EncFlush(&states[i], &cursor);
        require(size_t(end - cursor) * 4 == codec.bytes());
        require(std::equal(cursor, end, codec.word_storage.data() + codec.offset));
    } else if constexpr (Kind == 0) {
        std::vector<uint8_t> storage(input.size() * 2 + 8);
        auto* end = storage.data() + storage.size(); auto* cursor = end;
        std::array<RansState, 2> states;
        for (auto& state : states) RansEncInit(&state);
        for (size_t i = input.size(); i-- != 0;)
            RansEncPutSymbol(&states[i % 2], &cursor, &codec.byte_enc[input[i]]);
        for (size_t i = 2; i-- != 0;) RansEncFlush(&states[i], &cursor);
        require(size_t(end - cursor) == codec.bytes());
        require(std::equal(cursor, end, codec.byte_storage.data() + codec.offset));
    }
#ifdef DELAYED_CODING_HAVE_RANS_SIMD
    else {
        std::vector<uint16_t> storage(input.size() + 16);
        auto* end = storage.data() + storage.size(); auto* cursor = end;
        std::array<RansWordEnc, 8> states;
        for (auto& state : states) state = RansWordEncInit();
        for (size_t i = input.size(); i-- != 0;)
            RansWordEncPut(&states[i % 8], &cursor, codec.stats.cum_freqs[input[i]], codec.stats.freqs[input[i]]);
        for (size_t i = 8; i-- != 0;) RansWordEncFlush(&states[i], &cursor);
        require(size_t(end - cursor) * 2 == codec.bytes());
        require(std::equal(cursor, end, codec.simd_storage.data() + codec.offset));
    }
#endif
    std::vector<Symbol> output(input.size());
    codec.decode(output); require(input == output);
}

template<class Symbol, unsigned Kind>
static void calibration_row(const char* name, const Model& model,
                            const std::vector<uint32_t>& source, unsigned bits) {
    std::vector<Symbol> input(source.begin(), source.end()), output(source.size());
    OriginalRans<Kind, Kind == 2 ? 12 : 14> codec(model, source.size());
    validate_reference(codec, input);
    if (validate_only) return;
    const size_t repeats = std::max<size_t>(1, 4194304 / input.size());
    auto enc = median_ns(repeats, [&] { codec.encode(input); checksum += codec.bytes(); });
    auto dec = median_ns(repeats, [&] { codec.decode(output); checksum += output[input.size()/2]; });
    require(input == output);
    std::cout << name << ',' << bits << ',' << sizeof(Symbol)*8 << ',' << input.size() << ','
              << codec.bytes() << ',' << enc/input.size() << ',' << dec/input.size() << '\n';
}

static void calibrate(const std::vector<uint32_t>& source) {
    const std::vector<uint8_t> bytes(source.begin(), source.end());
    for (unsigned bits : {14u, 12u}) {
        original_rans::SymbolStats stats{};
        stats.count_freqs(bytes.data(), bytes.size());
        stats.normalize_freqs(1u << bits); // exact upstream policy, not ours
        std::array<uint32_t,256> weights{};
        for (unsigned i=0; i<256; ++i) weights[i] = stats.freqs[i] << (16-bits);
        const Model model(weights);
        if (bits == 14) {
            calibration_row<uint8_t,0>("upstream_byte_2", model, source, bits);
            calibration_row<uint32_t,0>("upstream_byte_2", model, source, bits);
            calibration_row<uint8_t,1>("upstream_rans64_2", model, source, bits);
            calibration_row<uint32_t,1>("upstream_rans64_2", model, source, bits);
        }
#ifdef DELAYED_CODING_HAVE_RANS_SIMD
        else if (*std::max_element(stats.freqs, stats.freqs+256) < 4096) {
            calibration_row<uint8_t,2>("upstream_sse41_8", model, source, bits);
            calibration_row<uint32_t,2>("upstream_sse41_8", model, source, bits);
        }
#endif
    }
}

int main(int argc, char** argv) {
    try {
#ifdef DELAYED_CODING_HAVE_RANS_SIMD
        if (!__builtin_cpu_supports("sse4.1")) throw std::runtime_error("SSE4.1 CPU required");
#endif
        std::cout << "codec,probability_bits,io_bits,symbols,payload_bytes,encode_ns_per_symbol,decode_ns_per_symbol\n";
        std::cout << std::fixed << std::setprecision(4);
        if (argc == 2 && std::string(argv[1]) == "--check") {
            validate_only = true;
            std::array<uint32_t,256> weights; weights.fill(256);
            const Model fixed(weights);
            const std::vector<uint32_t> empty;
            OriginalRans<0,14> empty_byte(fixed, 0);
            OriginalRans<1,14> empty_word(fixed, 0);
            validate_reference(empty_byte, empty); validate_reference(empty_word, empty);
#ifdef DELAYED_CODING_HAVE_RANS_SIMD
            OriginalRans<2> empty_simd(fixed, 0);
            validate_reference(empty_simd, empty);
#endif
            for (unsigned n : {1,2,3,4,5,6,7,8,9,15,16,17,4097}) {
                for (unsigned alphabet : {1,2,16,256}) {
                    std::vector<uint32_t> input(n);
                    for (unsigned i=0; i<n; ++i) input[i] = (i*1337) % alphabet;
                    calibrate(input);
                }
            }
        } else if (argc == 2) calibrate(read_symbols(argv[1]));
        else throw std::runtime_error("usage: calibrate_upstream FILE | --check");
        std::cerr << "calibration roundtrips and scalar-reference bytes verified\n";
    } catch (const std::exception& error) { std::cerr << error.what() << '\n'; return 1; }
}
