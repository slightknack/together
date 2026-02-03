#!/usr/bin/env bash
set -e

# AFL Fuzzing script for Together RGA
#
# Prerequisites:
#   brew install afl-fuzz   # or cargo install afl
#   cargo install cargo-afl
#
# Usage:
#   ./fuzz.sh          # Start fuzzing
#   ./fuzz.sh build    # Just build the fuzz target
#   ./fuzz.sh clean    # Clean fuzz output

case "${1:-run}" in
    build)
        echo "Building fuzz target..."
        cargo afl build --features="afl" --bin fuzz_rga
        echo "Build complete: target/debug/fuzz_rga"
        ;;

    clean)
        echo "Cleaning fuzz output..."
        rm -rf fuzz/out
        echo "Done"
        ;;

    run|*)
        echo "Building fuzz target..."
        cargo afl build --features="afl" --bin fuzz_rga

        echo ""
        echo "Starting AFL fuzzer..."
        echo "  Input corpus: fuzz/corpus/"
        echo "  Output: fuzz/out/"
        echo ""
        echo "Press Ctrl+C to stop"
        echo ""

        mkdir -p fuzz/out
        cargo afl fuzz -i fuzz/corpus -o fuzz/out target/debug/fuzz_rga
        ;;
esac
