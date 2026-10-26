// Instrumentation for causal-triangulations#258; links the pinned upstream kernels.
#include "simulation.hpp"
#include <fstream>
#include <iostream>
#include <stdexcept>
#include <unordered_map>

static void snapshot(const std::string& prefix, const std::array<int, 4>& counts) {
    Universe::check();
    Universe::updateVertexData();
    Universe::updateTriangleData();
    Universe::updateLinkData();
    Universe::exportGeometry(prefix + ".dat");
    std::unordered_map<int, int> vertices, triangles;
    for (auto v : Universe::vertices) vertices.emplace(v, vertices.size());
    for (auto t : Universe::triangles) triangles.emplace(t, triangles.size());
    std::ofstream out(prefix + ".json");
    out.exceptions(std::ios::badbit | std::ios::failbit);
    out << "{\"sphere\":" << (Universe::sphere ? "true" : "false")
        << ",\"move_counts_reject_add_delete_flip\":[";
    for (int i = 0; i < 4; ++i) out << (i ? "," : "") << counts[i];
    out << "],\"slice_sizes\":[";
    for (int i = 0; i < Universe::nSlices; ++i)
        out << (i ? "," : "") << Universe::sliceSizes.at(i);
    out << "],\"vertex_neighbors\":[";
    bool first = true;
    for (auto v : Universe::vertices) {
        out << (first ? "[" : ",["); first = false;
        bool inner = true;
        for (auto n : Universe::vertexNeighbors.at(v)) {
            out << (inner ? "" : ",") << vertices.at(n); inner = false;
        }
        out << "]";
    }
    out << "],\"triangle_neighbors\":[";
    first = true;
    for (auto t : Universe::triangles) {
        out << (first ? "[" : ",["); first = false;
        bool inner = true;
        for (auto n : Universe::triangleNeighbors.at(t)) {
            out << (inner ? "" : ",") << triangles.at(n); inner = false;
        }
        out << "]";
    }
    out << "],\"links\":[";
    first = true;
    for (auto l : Universe::links) {
        out << (first ? "[" : ",["); first = false;
        out << vertices.at(l->getVertexInitial()) << ","
            << vertices.at(l->getVertexFinal()) << "]";
    }
    out << "]}\n";
}

int main(int argc, char** argv) {
    if (argc != 4 && argc != 5) {
        std::cerr << "usage: census false|true OUTPUT_PREFIX ATTEMPTS [IMPORT.dat]\n";
        return 2;
    }
    if (std::string(argv[1]) != "false" && std::string(argv[1]) != "true") return 2;
    Universe::sphere = std::string(argv[1]) == "true";
    if (argc == 5) Universe::importGeometry(argv[4]);
    else Universe::create(4);
    // Use public start() solely to configure the upstream RNG/acceptance kernel.
    // Skips grow/thermalize and requests zero measurements; no equilibrium claim.
    Universe::imported = true;
    Simulation::start(0, 0.693147, 96, 1);
    const int attempts = std::stoi(argv[3]);
    if (attempts < 0 || attempts > 100000) return 2;
    std::array<int, 4> counts = {0, 0, 0, 0};
    snapshot(std::string(argv[2]) + "-initial", counts);
    for (int i = 0; i < attempts; ++i) {
        counts.at(Simulation::attemptMove())++;
        Universe::check();
    }
    if (attempts) {
        if (!counts[1] || !counts[2] || !counts[3])
            throw std::runtime_error("mixed accepted moves required");
        snapshot(std::string(argv[2]) + "-evolved", counts);
    }
}
