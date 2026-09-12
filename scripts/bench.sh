#!/usr/bin/env bash
# Times avdumpr (this port) against the original C# AVDump3CL on the given files, median of N runs,
# warm cache. Both run the same consumers and write the same AVD3 report.
#   scripts/bench.sh /path/to/AVDump3CL-build-dir file...
# The C# build dir must contain AVDump3CL.dll, AVDump3NativeLib.so and MediaInfo.so; needs `dotnet`.
set -euo pipefail
csdir=$1; shift
here=$(cd "$(dirname "$0")/.." && pwd)
rust="$here/target/release/avdumpr"
N=${N:-5}
CONS=${CONS:-ED2K,CRC32,MD5,SHA1,TTH,MKV,MP4}
out=$(mktemp -d); trap 'rm -rf "$out"' EXIT
common=(--Cons="$CONS" --Reports=AVD3 --RDir="$out" --HideBuffers --HideFileProgress --HideTotalProgress)

median_ms() {
  local t=()
  for _ in $(seq "$N"); do
    local s=$(date +%s%N); "$@" >/dev/null 2>&1; local e=$(date +%s%N)
    t+=($(( (e - s) / 1000 )))
  done
  printf '%s\n' "${t[@]}" | sort -n | sed -n "$(( (N + 1) / 2 ))p"
}

printf '| File | Size | AVDump3 (C#, .NET 8) | avdumpr | Speed-up |\n|---|---|---|---|---|\n'
for f in "$@"; do
  size=$(du -h "$f" | cut -f1)
  ref=$(median_ms dotnet "$csdir/AVDump3CL.dll" "${common[@]}" "$f")
  ours=$(median_ms "$rust" "${common[@]}" "$f")
  awk -v f="$(basename "$f")" -v s="$size" -v r="$ref" -v o="$ours" 'BEGIN { printf "| %s | %s | %.0f ms | %.0f ms | %.1f× |\n", f, s, r / 1000, o / 1000, (o > 0 ? r / o : 0) }'
done
