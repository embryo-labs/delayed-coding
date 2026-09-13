// Differential oracle: link the original Blitzcrank utility.cpp without backend macros.
#include "delayed_coding.h"
#include "utility.h"
#include <algorithm>
#include <memory>
#include <random>
#include <stdexcept>

static void require(bool ok) { if (!ok) throw std::runtime_error("mixed branch mismatch"); }
using Handle = std::unique_ptr<DcBranch, decltype(&dc_branch_free)>;
static Handle import(const db_compress::Branch& branch) {
    std::vector<DcInterval> intervals;
    for (auto segment : branch.segments_) intervals.push_back({uint32_t(segment.left_prob_), uint32_t(segment.right_prob_)});
    DcBranch* result = nullptr;
    require(dc_branch_new(intervals.data(), intervals.size(), branch.total_weights_, &result) == DC_OK);
    return Handle(result, dc_branch_free);
}

int main() {
    static_assert(kDelayedCoding == 24);
    std::mt19937 random(123456);
    std::vector<unsigned> weights{1, 127, 4096, 28672, 32640};
    db_compress::DelayedCodingParams model;
    db_compress::InitDelayedCodingParams(weights, model);
    std::vector<Handle> imported;
    for (const auto& branch : model.branches_) imported.push_back(import(branch));
    DcWorkspace* workspace = nullptr;
    require(dc_workspace_new(&workspace) == DC_OK);
    for (int length : {0, 1, 2, 3, 4, 5, 17, 127, 4097}) {
        std::vector<std::unique_ptr<db_compress::Branch>> simple;
        std::vector<Handle> simple_imports;
        std::vector<db_compress::Branch*> branches;
        std::vector<const DcBranch*> handles;
        for (int i = 0; i < length; ++i) {
            if (i % 3 == 0) {
                const auto id = random() % weights.size();
                branches.push_back(&model.branches_[id]); handles.push_back(imported[id].get());
            } else {
                const unsigned f = i % 3 == 1 ? 21845 : 1;
                const auto id = random() % (65536 / f);
                simple.push_back(std::make_unique<db_compress::Branch>(f, db_compress::ProbInterval(id * f, (id + 1) * f)));
                branches.push_back(simple.back().get());
                simple_imports.push_back(import(*simple.back()));
                handles.push_back(simple_imports.back().get());
            }
        }
        for (unsigned lanes : {1, 4}) {
            // Encode each lane independently with the old implementation, then
            // merge physical words using the capacity-only forward schedule.
            std::vector<db_compress::BitString> bits;
            for (unsigned lane = 0; lane < lanes; ++lane) {
                std::vector<db_compress::Branch*> lane_branches;
                for (size_t i = lane; i < branches.size(); i += lanes) lane_branches.push_back(branches[i]);
                int n = lane_branches.size();
                bits.emplace_back(n + 1);
                std::vector<bool> virtuals(n);
                db_compress::DelayedCoding(lane_branches, n, &bits.back(), virtuals);
            }
            std::vector<uint64_t> capacity(lanes, 1);
            std::vector<size_t> positions(lanes, 0);
            std::vector<uint8_t> expected;
            for (size_t i = 0; i < branches.size(); ++i) {
                const unsigned lane = i % lanes;
                if (capacity[lane] >= 1u << 24) capacity[lane] >>= 16;
                else {
                    const auto& stream = bits[lane];
                    require(positions[lane] < stream.num_);
                    const auto word = stream.bits_[stream.size_ - stream.num_ + positions[lane]++];
                    expected.push_back(word >> 8); expected.push_back(word & 255);
                }
                capacity[lane] *= branches[i]->total_weights_;
            }
            for (unsigned lane = 0; lane < lanes; ++lane) require(positions[lane] == bits[lane].num_);
            std::vector<uint8_t> actual(length * 2 + 1, 0xa5);
            size_t offset = 0, size = 0;
            require(dc_encode_branches(handles.data(), handles.size(), 24, lanes,
                actual.data(), actual.size(), workspace, &offset, &size) == DC_OK);
            require(size == expected.size());
            require(std::equal(expected.begin(), expected.end(), actual.begin() + offset));
            require(std::all_of(actual.begin(), actual.begin() + offset, [](auto b) { return b == 0xa5; }));
        }
    }
    dc_workspace_free(workspace);
    std::cout << "18 mixed alias/interval/raw blocks match original scalar lane streams\n";
}
