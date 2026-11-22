#!/usr/bin/env bash
# Download the published EIP-8321 fixtures without regenerating or modifying them.
set -euo pipefail

preset=${1:?Usage: download_eip8321_vectors.sh mainnet|minimal}
case "$preset" in
    mainnet) checksum=09ab991ab138d579bce6dcdefbb79efc55e2527360784e955d8afa354798e1d9 ;;
    minimal) checksum=4696ba472246c386db94ef0f02bb21867231da952045a2cb79a81cd52cd605bb ;;
    *) echo "Unsupported preset: $preset" >&2; exit 1 ;;
esac

base_url=https://github.com/lukassta/consensus-specs/releases/download/v1.7.0-eip8321.1
output_dir=$(cd "$(dirname "$0")" && pwd)/consensus-spec-tests
archive_dir=$(mktemp -d)
trap 'rm -rf "$archive_dir"' EXIT
mkdir -p "$output_dir"

for name in general "$preset"; do
    curl --fail --location --show-error --retry 3 \
        "$base_url/$name.tar.gz" -o "$archive_dir/$name.tar.gz"
done
echo "c9310224146fe31a44627b97b3689ee5a8dd8a0b9401f80030e54045fd313248  $archive_dir/general.tar.gz" | sha256sum --check
echo "$checksum  $archive_dir/$preset.tar.gz" | sha256sum --check
tar -xzf "$archive_dir/general.tar.gz" -C "$output_dir"
# The dedicated test feature does not use historical fork fixtures. Avoid extracting
# several gigabytes of unrelated data on the GitHub-hosted runner.
tar -xzf "$archive_dir/$preset.tar.gz" -C "$output_dir" "tests/$preset/eip8321"
# Fail early if a release layout change would silently skip all feature cases.
test -d "$output_dir/tests/$preset/eip8321"
