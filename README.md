# AVDump3 (Rust port)

A one-shot Rust port of [AVDump3](https://github.com/DvdKhl/AVDump3) (`AVDump3CL` + `AVDump3Lib`).

AVDump3 reads every file **once** into a mirrored circular buffer and feeds the data to any number of
consumers in parallel — hash algorithms (ED2K, CRC32, MD5, SHA-1/2/3, Keccak, Tiger, TTH, …) and
container parsers (Matroska, Ogg, MP4) — then emits metadata reports (XML), side logs, and can
move/rename files based on the results. The command line, namespaces, argument names, aliases and
output formats mirror the original 1:1.

```
avdump3 --Consumers=ED2K,CRC32 --PrintHashes video.mkv
avdump3 -R --Cons=ED2K,MKV --Reports=AVD3 --RDir=out /media
avdump3 --Consumers            # list consumers
avdump3 --Help                 # full, coloured help; --Help=<NameSpace> for one namespace
```

## Building

```
cargo build --release          # binary: target/release/avdump3
cargo test --release           # unit + end-to-end tests
```

No native build steps: all hash algorithms are pure Rust (RustCrypto + crc32fast/crc32c) and the
mirrored buffer uses `memfd_create`/`mmap` on Linux (shm on other unixes, a copy-on-wrap buffer
elsewhere).

### MediaInfo

The `MediaInfoLibProvider` and the `MediaInfoXml` report use [MediaInfoLib](https://mediaarea.net/en/MediaInfo)
when it can be found at runtime (`libmediainfo.so.0` on the library path, a `MediaInfo-linux-x64.so`
next to the binary as shipped with the C# release, or the path in `$AVD3_MEDIAINFO`). Without it the
program still runs; `--Version` reports `MediaInfoLib: not available`.

On Arch: `sudo pacman -S libmediainfo`.

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

Run `avdump3 --Help` for descriptions, examples and defaults.

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
