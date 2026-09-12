# AVDumpR

Rust port of [AVDump3](https://github.com/DvdKhl/AVDump3) (`AVDump3CL` + `AVDump3Lib`): reads each
file once and feeds it to parallel hash consumers (ED2K, CRC32, MD5, SHA-1/2/3, Tiger, TTH, …) and
container parsers (Matroska, MP4, Ogg), then writes metadata reports and can move/rename files.
Same command line, argument names and output formats as the original; the binary is `avdumpr`. Media metadata comes from
[mediainfo-rust](https://github.com/sandwichfarm/mediainfo-rust), compiled in — no native libraries.

## Install

```
cargo install avdumpr                                        # crates.io
docker run --rm -v "$PWD:/data" ghcr.io/sandwichfarm/avdumpr --Cons=ED2K,CRC32 --PrintHashes video.mkv
```

Prebuilt binaries (Linux, macOS, Windows) are on the [releases page](https://github.com/sandwichfarm/avdump-rust/releases).
Docker: `/data` is the working directory; mount `:ro` unless you write reports/logs, add
`--user "$(id -u):$(id -g)"` to keep file ownership, `-it` for the live progress display.

## Use

```
avdumpr --Consumers=ED2K,CRC32 --PrintHashes video.mkv
avdumpr -R --Cons=ED2K,MKV --Reports=AVD3 --RDir=out /media
avdumpr --Consumers            # list consumers
avdumpr --Help                 # full help; --Help=<NameSpace> for one namespace
```

## Benchmark

Wall-clock per file for `--Cons=ED2K,CRC32,MD5,SHA1,TTH,MKV,MP4 --Reports=AVD3`, median of 5
warm-cache runs, i7-11700K (8 cores), Linux. The original is AVDump3CL built for .NET 8 with its
native hash library and MediaInfoLib 20.08. `scripts/bench.sh` reproduces the table.

| File | Size | AVDump3 (C#, .NET 8) | avdumpr | Speed-up |
|---|---|---|---|---|
| HandBrake MKV (AVC + AAC) | 34 MB | 1299 ms | 559 ms | 2.3× |
| 2 min 720p MKV (AVC + AAC) | 122 MB | 1716 ms | 748 ms | 2.3× |
| MP4 (AVC + AAC) | 75 MB | 1142 ms | 511 ms | 2.2× |
| Random data (hashing only) | 1 GiB | 2343 ms | 1892 ms | 1.2× |

## Develop

```
cargo build --release          # target/release/avdumpr
cargo test --release
docker build --ssh default -t avdumpr .   # needs SSH read access to mediainfo-rust (private)
```

`mediainfo-rust` is a git dependency pinned by revision (fetched over SSH, see `.cargo/config.toml`).
Releases: push a `vX.Y.Z` tag — CI publishes to crates.io, GHCR and the releases page.
Secrets (`CARGO_REGISTRY_TOKEN`, `MEDIAINFO_RUST_DEPLOY_KEY`): run `scripts/setup-secrets.sh`.

## Command line

Arguments are `--Name`, `--Name=Value`, `--NameSpace.Name=Value`, `-X` (single-letter aliases) or
`-RXY` (several single-letter switches). Names are case-insensitive. `FROMFILE <path>` reads
arguments from a file (one per line, `//` comments). `PRINTARGS` echoes the parsed arguments.

| NameSpace | Arguments |
|--|--|
| FileDiscovery | `--Recursive/-R`, `--ProcessedLogPath/--PLPath`, `--SkipLogPath/--SLPath`, `--DoneLogPath/--DLPath`, `--WithExtensions/--WExts`, `--Concurrent/--Conc` |
| Processing | `--ProducerMinReadLength`, `--ProducerMaxReadLength`, `--PrintAvailableSIMDs`, `--PauseBeforeExit/--PBExit`, `--BufferLength/--BLength`, `--Consumers/--Cons` |
| FileMove | `--FileMove.Test`, `--FileMove.LogPath`, `--FileMove.Mode`, `--FileMove.Pattern`, `--FileMove.DisableFileMove`, `--FileMove.DisableFileRename`, `--FileMove.Replacements` |
| Reporting | `--PrintHashes`, `--PrintReports`, `--Reports`, `--ReportDirectory/--RDir`, `--ReportFileName`, `--ReportContentPrefix`, `--ExtensionDifferencePath/--EDPath`, `--CRC32Error` |
| Diagnostics | `--Version`, `--SaveErrors`, `--SkipEnvironmentElement`, `--IncludePersonalData`, `--PrintDiscoveredFiles`, `--ErrorDirectory`, `--NullStreamTest` |
| Display | `--HideBuffers`, `--HideFileProgress`, `--HideTotalProgress`, `--ShowDisplayJitter`, `--ForwardConsoleCursorOnly` |

Run `avdumpr --Help` for descriptions, examples and defaults.

### Consumers

`CPY`, `CRC32`, `CRC32C`, `ED2K`, `KECCAK-224/256/384/512`, `MD4`, `MD5`, `MKV`, `MP4`, `NULL`,
`OGG`, `SHA1`, `SHA2-256/384/512`, `SHA3-224/256/384/512`, `TIGER`, `TTH`.
Per-consumer arguments: `--Consumers=TTH:4,NULL:8,CPY:/target/dir`.

### Reports

`AVD3` (complete metadata tree), `Matroska` (EBML structure dump), `MediaInfoXml` (raw MediaInfo).

### Placeholders

`--FileMove.Pattern`, `--ReportFileName` and `--ReportContentPrefix` accept `${Name}` placeholders:
`FileSize`, `FullName`, `FileName`, `FileExtension`, `FileNameWithoutExtension`, `DirectoryName`,
`SuggestedExtension`, `Hash-<Consumer>-<2|4|8|10|16|32|32Hex|32Z|36|62|64>-<OC|UC|LC>` and (reports
only) `ReportName`, `ReportFileExtension`.

## Differences from the C# original

* `--FileMove.Mode=CSharpScriptInline|CSharpScriptFile|DotNetAssembly` are accepted for command
  compatibility but rejected at startup with a clear message — there is no C# compiler to embed.
  `PlaceholderInline` and `PlaceholderFile` (including `--FileMove.Test` interactive mode) work.
* The TTH of an empty file is the standard value (`LWPNACQDBZRYXW3VHJVCJ64QBZNGHOHHHZWCLNQ`) instead
  of the plain Tiger digest of an empty string.
* Matroska `CueCount` compares cue track numbers against the track number (the original compared
  against the track UID and therefore always reported 0).
* Consumers that fail after three attempts are left out of the metadata instead of producing
  undefined values; the error is printed (and saved with `--SaveErrors`).
* The live progress display is disabled automatically when stdout is not a terminal.
* Exit codes: `0` success/informational, `1` configuration or processing error, `2` argument parse
  error, `130` cancelled with Ctrl+C.

## License

MIT, like the original (see LICENSE).

## Releasing

Tag the commit as `vX.Y.Z` (matching `Cargo.toml`) and push the tag: the `Release` workflow runs the
tests, publishes `avdumpr` to crates.io (`CARGO_REGISTRY_TOKEN` secret) and attaches Linux, macOS and
Windows binaries to the GitHub release. `cargo publish` uses the crates.io release of `mediainfo-rust`
pinned in `Cargo.toml`, so that crate has to be published first. CI needs read access to the
mediainfo-rust repository while it is private (`MEDIAINFO_RUST_DEPLOY_KEY` secret).
`scripts/setup-secrets.sh` prompts for both secrets and sets them with `gh` (it can also generate the
deploy key pair and register it on the mediainfo-rust repository).
