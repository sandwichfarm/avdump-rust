//! End-to-end tests running the `avdump3` binary.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bin() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_avdump3"));
    c.env("NO_COLOR", "1");
    c
}

fn run(args: &[&str]) -> Output {
    bin().args(args).output().expect("run avdump3")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn tmpdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("avdump3-test-{}-{}", name, std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn write(path: &Path, data: &[u8]) -> String {
    fs::write(path, data).unwrap();
    path.to_string_lossy().into_owned()
}

// ------------------------------------------------------------------ help & listings

#[test]
fn no_args_prints_help() {
    let o = run(&[]);
    assert!(o.status.success());
    let s = stdout(&o);
    assert!(s.contains("USAGE"));
    for ns in ["FileDiscovery", "Processing", "FileMove", "Reporting", "Diagnostics", "Display"] {
        assert!(s.contains(&format!("NameSpace: {ns}")), "missing namespace {ns}");
    }
    assert!(s.contains("Use --Help OR --Help=<NameSpace> for more detailed info"));
}

#[test]
fn explicit_help_is_detailed_and_topic_filters() {
    let s = stdout(&run(&["--Help"]));
    assert!(s.contains("Recursively descent into Subdirectories"));
    let s = stdout(&run(&["--Help=Display"]));
    assert!(s.contains("--HideBuffers"));
    assert!(!s.contains("--Recursive"));
    let s = stdout(&run(&["--help=nothing"]));
    assert!(s.contains("There is no such topic"));
    let s = stdout(&run(&["-h"]));
    assert!(s.contains("USAGE"));
}

#[test]
fn lists_consumers_and_reports() {
    let s = stdout(&run(&["--Consumers"]));
    assert!(s.starts_with("Available Consumers: "));
    for c in ["CRC32", "ED2K", "MD5", "SHA1", "TTH", "MKV", "OGG", "MP4", "NULL", "CPY", "KECCAK-512", "SHA3-224", "TIGER", "CRC32C", "MD4", "SHA2-512"] {
        assert!(s.contains(&format!("{c:<14} - ")), "missing consumer {c}");
    }
    let s = stdout(&run(&["--Reports"]));
    assert!(s.contains("AVD3           - "));
    assert!(s.contains("MediaInfoXml   - "));
    assert!(s.contains("Matroska       - "));
}

#[test]
fn invalid_arguments_are_reported() {
    let o = run(&["--Consumers=NOPE", "x"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stdout(&o).contains("Invalid BlockConsumer(s): NOPE"));
    let o = run(&["--Reports=NOPE", "--Consumers=CRC32", "x"]);
    assert!(stdout(&o).contains("Invalid Report: NOPE"));
    let o = run(&["--Bogus"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(stdout(&o).contains("Argument (Bogus) is not registered"));
}

#[test]
fn version_and_simd() {
    let s = stdout(&run(&["--Version"]));
    assert!(s.contains("Program Version: "));
    let s = stdout(&run(&["--PrintAvailableSIMDs"]));
    assert!(s.starts_with("Available SIMD Instructions: "));
}

#[test]
fn printargs_and_fromfile() {
    let d = tmpdir("fromfile");
    let f = write(&d.join("args.txt"), b"// comment\n--Consumers\n\n");
    let s = stdout(&bin().args(["FROMFILE", &f, "PRINTARGS"]).output().unwrap());
    assert!(s.starts_with("--Consumers\n"));
    assert!(s.contains("Available Consumers"));
    let o = run(&["FROMFILE", "/nonexistent/args"]);
    assert!(stdout(&o).contains("FROMFILE: File not found"));
}

// ------------------------------------------------------------------ hashing

#[test]
fn hashes_match_reference_values() {
    let d = tmpdir("hashes");
    let f = write(&d.join("abc.bin"), b"abc");
    let z = write(&d.join("zeros.bin"), &vec![0u8; 9_728_000]);
    let o = bin()
        .args(["--Consumers=CRC32,CRC32C,ED2K,MD4,MD5,SHA1,SHA2-256,SHA3-256,KECCAK-256,TIGER,TTH", "--PrintHashes", "--ForwardConsoleCursorOnly", &f, &z])
        .output()
        .unwrap();
    assert!(o.status.success());
    let s = stdout(&o);
    assert!(s.contains("abc.bin\n"));
    assert!(s.contains("CRC32 => 352441C2"));
    assert!(s.contains("CRC32C => 364B3FB7"));
    assert!(s.contains("MD5 => 900150983CD24FB0D6963F7D28E17F72"));
    assert!(s.contains("SHA1 => A9993E364706816ABA3E25717850C26C9CD0D89D"));
    assert!(s.contains("TIGER => 2AAB1484E8C158F2BFB8C5FF41B57A525129131C957B5F93"));
    // ED2K of exactly one chunk of zeros yields red and blue variants.
    assert!(s.contains("ED2K => FC21D9AF828F92A8DF64BEAC3357425D"));
    assert!(s.contains("ED2K2 => D7DEF262A127CD79096A108E7A9FC138"));
    assert!(s.contains("2/2 Files"));
}

#[test]
fn null_stream_test_runs_without_files() {
    let o = run(&["--NullStreamTest=3:8:2", "--Consumers=NULL,CRC32", "--ForwardConsoleCursorOnly"]);
    assert!(o.status.success());
    assert!(stdout(&o).contains("3/3 Files | 24/24 MiB"));
    let o = run(&["--NullStreamTest=1:1:1", "--Consumers=CRC32", "--Reports=AVD3"]);
    assert!(stdout(&o).contains("NullStreamTest cannot be used with reports"));
}

#[test]
fn copy_consumer_writes_a_copy() {
    let d = tmpdir("cpy");
    let src = write(&d.join("in.dat"), &(0..100_000u32).map(|i| i as u8).collect::<Vec<_>>());
    let out = d.join("copies");
    let o = bin().args([&format!("--Consumers=CPY:{},CRC32", out.display()), "--ForwardConsoleCursorOnly", &src]).output().unwrap();
    assert!(o.status.success(), "{}", stdout(&o));
    assert_eq!(fs::read(out.join("in.dat")).unwrap(), fs::read(&src).unwrap());
}

// ------------------------------------------------------------------ discovery & logs

#[test]
fn recursion_extensions_and_logs() {
    let d = tmpdir("discovery");
    fs::create_dir_all(d.join("sub/deeper")).unwrap();
    write(&d.join("a.mkv"), b"aaaa");
    write(&d.join("b.avi"), b"bbbb");
    write(&d.join("sub/c.mkv"), b"cccc");
    write(&d.join("sub/deeper/d.mkv"), b"dddd");
    let dir = d.join("sub").to_string_lossy().into_owned();
    let done = d.join("done.log").to_string_lossy().into_owned();
    let dir = {
        // Scan a directory that does not contain the log itself.
        fs::create_dir_all(d.join("scan")).unwrap();
        for name in ["a.mkv", "b.avi", "sub/c.mkv", "sub/deeper/d.mkv"] {
            let target = d.join("scan").join(name);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::rename(d.join(name), &target).unwrap();
        }
        let _ = dir;
        d.join("scan").to_string_lossy().into_owned()
    };

    let s = stdout(&run(&["--Consumers=CRC32", "--WExts=.mkv", "--PrintDiscoveredFiles", "--ForwardConsoleCursorOnly", &dir]));
    assert!(s.contains("Accepted file: ") && s.contains("a.mkv") && !s.contains("b.avi") && !s.contains("c.mkv"));

    let s = stdout(&run(&["--Consumers=CRC32", "-R", "--WExts=-.avi", &format!("--DoneLogPath={done}"), "--ForwardConsoleCursorOnly", &dir]));
    assert!(s.contains("Accepted files: 3"));
    let logged = fs::read_to_string(&done).unwrap();
    assert_eq!(logged.lines().count(), 3);
    assert!(logged.contains("deeper/d.mkv") || logged.contains("deeper\\d.mkv"));

    // Second run skips everything listed in the done log.
    let s = stdout(&run(&["--Consumers=CRC32", "-R", &format!("--DLPath={done}"), "--ForwardConsoleCursorOnly", &dir]));
    assert!(s.contains("Accepted files: 1"), "{s}");

    let o = run(&["--Consumers=CRC32", "--SkipLogPath=/nonexistent/skip.log", &dir]);
    assert!(stdout(&o).contains("SkipLogPath contains file paths which do not exist"));
    let o = run(&["--Consumers=CRC32", "--ForwardConsoleCursorOnly", "/nonexistent/path"]);
    assert!(stdout(&o).contains("Filediscovery: Path not found"));
}

#[test]
fn crc32_error_and_extension_difference_logs() {
    let d = tmpdir("sidelogs");
    let ok = write(&d.join("good [352441C2].txt"), b"abc");
    let bad = write(&d.join("bad [DEADBEEF].txt"), b"abc");
    let srt = write(&d.join("subs.srt"), b"1\n00:00:01,000 --> 00:00:02,000\nHi\n\n2\n00:00:03,000 --> 00:00:04,000\nThere\n");
    let crc_log = d.join("crc.log").to_string_lossy().into_owned();
    let ed_log = d.join("ext.log").to_string_lossy().into_owned();
    // CRC32 gets force-enabled by --CRC32Error even though only MD5 is selected.
    let o = bin().args(["--Consumers=MD5", &format!("--CRC32Error={crc_log}"), &format!("--EDPath={ed_log}"), "--PrintHashes", "--ForwardConsoleCursorOnly", &ok, &bad, &srt]).output().unwrap();
    assert!(o.status.success());
    assert!(stdout(&o).contains("CRC32 => 352441C2"));
    let crc = fs::read_to_string(&crc_log).unwrap();
    assert!(crc.contains("bad [DEADBEEF].txt"));
    assert!(!crc.contains("good [352441C2].txt"));
    let ed = fs::read_to_string(&ed_log).unwrap();
    assert!(ed.contains("txt => unknown\t"));
    assert!(!ed.contains("subs.srt"), "srt is detected correctly: {ed}");
}

// ------------------------------------------------------------------ reports

#[test]
fn avd3_report_is_saved_with_placeholders() {
    let d = tmpdir("report");
    let f = write(&d.join("clip.bin"), b"hello world");
    let rdir = d.join("reports");
    let o = bin()
        .args([
            "--Consumers=CRC32,ED2K",
            "--Reports=AVD3",
            &format!("--RDir={}", rdir.display()),
            "--ReportFileName=${FileNameWithoutExtension}_${Hash-CRC32-16-LC}.${ReportName}.${ReportFileExtension}",
            "--ReportContentPrefix=# ${FileName}",
            "--PrintReports",
            "--ForwardConsoleCursorOnly",
            &f,
        ])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", stdout(&o));
    let printed = stdout(&o);
    assert!(printed.contains("<FileInfo>"));
    let report = rdir.join("clip_0d4a1185.AVD3.xml");
    assert!(report.exists(), "report file missing; dir: {:?}", fs::read_dir(&rdir).map(|r| r.map(|e| e.unwrap().file_name()).collect::<Vec<_>>()));
    let xml = fs::read_to_string(&report).unwrap();
    assert!(xml.starts_with("# clip.bin\n<FileInfo>"));
    assert!(xml.contains("<Size>11</Size>"));
    assert!(xml.contains("<CRC32 p=\"HashProvider\" t=\"Binary\" u=\"Dimensionsless\">0D4A1185</CRC32>"));
    assert!(xml.contains("<ED2K p=\"HashProvider\""));
    assert!(xml.contains("<MediaProvider"));
    assert!(xml.trim_end().ends_with("</FileInfo>"));
}

// ------------------------------------------------------------------ container parsers

/// Minimal EBML writer for building a synthetic Matroska file.
fn ebml(id: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let id_bytes = id.to_be_bytes();
    let start = id_bytes.iter().position(|b| *b != 0).unwrap_or(3);
    out.extend_from_slice(&id_bytes[start..]);
    let len = payload.len() as u64;
    if len < 0x7F {
        out.push(0x80 | len as u8);
    } else if len < 0x3FFF {
        out.push(0x40 | (len >> 8) as u8);
        out.push(len as u8);
    } else {
        out.push(0x10 | (len >> 24) as u8);
        out.extend_from_slice(&(len as u32).to_be_bytes()[1..]);
    }
    out.extend_from_slice(payload);
    out
}
fn uint(id: u32, v: u64) -> Vec<u8> {
    let b = v.to_be_bytes();
    let start = b.iter().position(|x| *x != 0).unwrap_or(7);
    ebml(id, &b[start..])
}
fn float(id: u32, v: f64) -> Vec<u8> {
    ebml(id, &v.to_be_bytes())
}
fn string(id: u32, s: &str) -> Vec<u8> {
    ebml(id, s.as_bytes())
}
fn cat(parts: &[Vec<u8>]) -> Vec<u8> {
    parts.concat()
}

fn synthetic_mkv() -> Vec<u8> {
    let header = ebml(0x1A45DFA3, &cat(&[uint(0x4286, 1), uint(0x42F7, 1), string(0x4282, "matroska"), uint(0x4287, 4), uint(0x4285, 2)]));
    let info = ebml(0x1549A966, &cat(&[uint(0x2AD7B1, 1_000_000), float(0x4489, 2000.0), string(0x4D80, "synth-mux"), string(0x5741, "synth-write"), ebml(0x73A4, &[0xAB; 16])]));
    let video = ebml(0xE0, &cat(&[uint(0xB0, 640), uint(0xBA, 480), uint(0x54B0, 640), uint(0x54BA, 360)]));
    let track1 = ebml(0xAE, &cat(&[uint(0xD7, 1), uint(0x73C5, 1001), uint(0x83, 1), string(0x86, "V_TEST"), string(0x22B59C, "eng"), uint(0x23E383, 40_000_000), video]));
    let audio = ebml(0xE1, &cat(&[float(0xB5, 48000.0), uint(0x9F, 2), uint(0x6264, 16)]));
    let track2 = ebml(0xAE, &cat(&[uint(0xD7, 2), uint(0x73C5, 1002), uint(0x83, 2), string(0x86, "A_TEST"), string(0x536E, "Stereo"), audio]));
    let tracks = ebml(0x1654AE6B, &cat(&[track1, track2]));
    let mut blocks = Vec::new();
    for i in 0..50u16 {
        // SimpleBlock: track 1 (vint 0x81), timecode, flags, then 100 payload bytes.
        let mut b = vec![0x81];
        b.extend_from_slice(&(i * 40).to_be_bytes());
        b.push(0x80);
        b.extend_from_slice(&[i as u8; 100]);
        blocks.push(ebml(0xA3, &b));
        let mut a = vec![0x82];
        a.extend_from_slice(&(i * 40).to_be_bytes());
        a.push(0x00);
        a.extend_from_slice(&[7u8; 20]);
        blocks.push(ebml(0xA3, &a));
    }
    let cluster = ebml(0x1F43B675, &cat(&[uint(0xE7, 0), cat(&blocks)]));
    let chapters = ebml(
        0x1043A770,
        &ebml(0x45B9, &cat(&[uint(0x45BC, 77), ebml(0xB6, &cat(&[uint(0x73C4, 1), uint(0x91, 0), uint(0x92, 1_000_000_000), ebml(0x80, &cat(&[string(0x85, "Opening"), string(0x437C, "eng")]))]))])),
    );
    let tags = ebml(0x1254C367, &ebml(0x7373, &cat(&[ebml(0x63C0, &uint(0x63C5, 1001)), ebml(0x67C8, &cat(&[string(0x45A3, "TITLE"), string(0x4487, "Synthetic")]))])));
    let attachments = ebml(0x1941A469, &ebml(0x61A7, &cat(&[string(0x466E, "cover.txt"), string(0x4660, "text/plain"), uint(0x46AE, 5), ebml(0x465C, b"hello")])));
    let segment = ebml(0x18538067, &cat(&[info, tracks, chapters, tags, attachments, cluster]));
    cat(&[header, segment])
}

#[test]
fn matroska_parser_reports_structure() {
    let d = tmpdir("mkv");
    let f = write(&d.join("synthetic.mkv"), &synthetic_mkv());
    let o = run(&["--Consumers=MKV,CRC32", "--Reports=AVD3,Matroska", "--PrintReports", "--ForwardConsoleCursorOnly", &f]);
    assert!(o.status.success());
    let s = stdout(&o);
    // AVD3 report (MatroskaProvider)
    assert!(s.contains("<ContainerVersion p=\"MatroskaProvider\" t=\"String\" u=\"Dimensionsless\">DocType=matroska DocTypeVersion=4</ContainerVersion>"), "{s}");
    assert!(s.contains("<Duration p=\"MatroskaProvider\" t=\"Double\" u=\"s\">2</Duration>"));
    assert!(s.contains("<Item>mkv</Item>"));
    assert!(s.contains("<VideoStream>"));
    assert!(s.contains("<PixelDimensions p=\"MatroskaProvider\" t=\"Dimensions\" u=\"Dimensionsless\">640, 480</PixelDimensions>"));
    assert!(s.contains("<DisplayDimensions p=\"MatroskaProvider\" t=\"Dimensions\" u=\"Dimensionsless\">640, 360</DisplayDimensions>"));
    assert!(s.contains("<StatedSampleRate p=\"MatroskaProvider\" t=\"Double\" u=\"s^-1\">25</StatedSampleRate>"));
    assert!(s.contains("<SampleCount p=\"MatroskaProvider\" t=\"Int64\" u=\"Dimensionsless\">50</SampleCount>"));
    assert!(s.contains("<Id p=\"MatroskaProvider\" t=\"UInt64\" u=\"Dimensionsless\">1001</Id>"));
    assert!(s.contains("<AudioStream>"));
    assert!(s.contains("<ChannelCount p=\"MatroskaProvider\" t=\"Int32\" u=\"Dimensionsless\">2</ChannelCount>"));
    assert!(s.contains("<Title p=\"MatroskaProvider\" t=\"String\" u=\"Dimensionsless\">Stereo</Title>"));
    assert!(s.contains("<Item>Opening Languages(eng) Countries()</Item>"));
    assert!(s.contains("Tags(TITLE=Synthetic)"));
    assert!(s.contains("<Attachment>"));
    assert!(s.contains("<Type p=\"MatroskaProvider\" t=\"String\" u=\"Dimensionsless\">text/plain</Type>"));
    // Matroska structure report
    assert!(s.contains("<EbmlHeader>"));
    assert!(s.contains("<TrackNumber>2</TrackNumber>"));
    assert!(s.contains("<CodecId>A_TEST</CodecId>"));
    assert!(s.contains("<MuxingApp>synth-mux</MuxingApp>"));
    assert!(s.contains("<SegmentUId Size=\"16\">ABABABABABABABABABABABABABABABAB</SegmentUId>"));
    assert!(s.contains("<FileDataSize>5</FileDataSize>"));
}

#[test]
fn matroska_parser_ignores_non_matroska() {
    let d = tmpdir("notmkv");
    let f = write(&d.join("plain.bin"), &vec![0x42u8; 5000]);
    let o = run(&["--Consumers=MKV", "--Reports=Matroska", "--PrintReports", "--ForwardConsoleCursorOnly", &f]);
    assert!(o.status.success());
    assert!(stdout(&o).contains("<File />"));
}

fn ogg_page(stream_id: u32, page_index: u32, flags: u8, granule: i64, packet: &[u8]) -> Vec<u8> {
    let mut p = b"OggS".to_vec();
    p.push(0);
    p.push(flags);
    p.extend_from_slice(&granule.to_le_bytes());
    p.extend_from_slice(&stream_id.to_le_bytes());
    p.extend_from_slice(&page_index.to_le_bytes());
    p.extend_from_slice(&[0, 0, 0, 0]);
    let mut segments = Vec::new();
    let mut rest = packet.len();
    while rest >= 255 {
        segments.push(255u8);
        rest -= 255;
    }
    segments.push(rest as u8);
    p.push(segments.len() as u8);
    p.extend_from_slice(&segments);
    p.extend_from_slice(packet);
    p
}

#[test]
fn ogg_parser_reads_vorbis_stream() {
    let mut ident = b"\x01vorbis".to_vec();
    ident.extend_from_slice(&0u32.to_le_bytes()); // version
    ident.push(2); // channels
    ident.extend_from_slice(&48000u32.to_le_bytes());
    ident.extend_from_slice(&[0u8; 14]); // bitrates, blocksizes, framing
    let mut comment = b"\x03vorbis".to_vec();
    let vendor = b"synthetic";
    comment.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    comment.extend_from_slice(vendor);
    comment.extend_from_slice(&2u32.to_le_bytes());
    for c in [&b"TITLE=Synthetic Song"[..], &b"LANGUAGE=eng"[..]] {
        comment.extend_from_slice(&(c.len() as u32).to_le_bytes());
        comment.extend_from_slice(c);
    }
    let mut file = ogg_page(7, 0, 2, 0, &ident);
    file.extend_from_slice(&ogg_page(7, 1, 0, 0, &comment));
    for i in 0..10 {
        file.extend_from_slice(&ogg_page(7, 2 + i, 0, ((i + 1) * 4800) as i64, &[0xAAu8; 300]));
    }
    let d = tmpdir("ogg");
    let f = write(&d.join("synthetic.ogg"), &file);
    let s = stdout(&run(&["--Consumers=OGG", "--Reports=AVD3", "--PrintReports", "--ForwardConsoleCursorOnly", &f]));
    assert!(s.contains("<Item>ogg</Item>"), "{s}");
    assert!(s.contains("<AudioStream>"));
    assert!(s.contains("<ChannelCount p=\"OggProvider\" t=\"Int32\" u=\"Dimensionsless\">2</ChannelCount>"));
    assert!(s.contains("<StatedSampleRate p=\"OggProvider\" t=\"Double\" u=\"s^-1\">48000</StatedSampleRate>"));
    assert!(s.contains("<SampleCount p=\"OggProvider\" t=\"Int64\" u=\"Dimensionsless\">48000</SampleCount>"));
    assert!(s.contains("<Duration p=\"OggProvider\" t=\"TimeSpan\" u=\"s\">00:00:01</Duration>"));
    assert!(s.contains("<Title p=\"OggProvider\" t=\"String\" u=\"Dimensionsless\">Synthetic Song</Title>"));
    assert!(s.contains("<Language p=\"OggProvider\" t=\"String\" u=\"Dimensionsless\">eng</Language>"));
    assert!(s.contains("<ContainerCodecId p=\"OggProvider\" t=\"String\" u=\"Dimensionsless\">Vorbis</ContainerCodecId>"));
}

fn mp4_box(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut b = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
    b.extend_from_slice(kind);
    b.extend_from_slice(payload);
    b
}

#[test]
fn mp4_parser_reads_boxes() {
    let ftyp = mp4_box(b"ftyp", b"isom\0\0\x02\0isomiso2avc1mp41");
    let mut mvhd = vec![0u8; 100];
    mvhd[12..16].copy_from_slice(&1000u32.to_be_bytes());
    mvhd[16..20].copy_from_slice(&3500u32.to_be_bytes());
    let mut tkhd = vec![0u8; 84];
    tkhd[12..16].copy_from_slice(&1u32.to_be_bytes()); // track id
    tkhd[76..80].copy_from_slice(&(320u32 << 16).to_be_bytes());
    tkhd[80..84].copy_from_slice(&(240u32 << 16).to_be_bytes());
    let mut mdhd = vec![0u8; 24];
    mdhd[12..16].copy_from_slice(&90000u32.to_be_bytes());
    mdhd[16..20].copy_from_slice(&315000u32.to_be_bytes());
    mdhd[20..22].copy_from_slice(&(((b'e' - 0x60) as u16) << 10 | ((b'n' - 0x60) as u16) << 5 | (b'g' - 0x60) as u16).to_be_bytes());
    let mut hdlr = vec![0u8; 24];
    hdlr[8..12].copy_from_slice(b"vide");
    let mut entry = vec![0u8; 78];
    entry[24..26].copy_from_slice(&320u16.to_be_bytes());
    entry[26..28].copy_from_slice(&240u16.to_be_bytes());
    entry[40..42].copy_from_slice(&1u16.to_be_bytes());
    let sample_entry = mp4_box(b"avc1", &entry);
    let mut stsd = vec![0, 0, 0, 0, 0, 0, 0, 1];
    stsd.extend_from_slice(&sample_entry);
    let stbl = mp4_box(b"stbl", &mp4_box(b"stsd", &stsd));
    let minf = mp4_box(b"minf", &stbl);
    let mdia = mp4_box(b"mdia", &[mp4_box(b"mdhd", &mdhd), mp4_box(b"hdlr", &hdlr), minf].concat());
    let trak = mp4_box(b"trak", &[mp4_box(b"tkhd", &tkhd), mdia].concat());
    let moov = mp4_box(b"moov", &[mp4_box(b"mvhd", &mvhd), trak].concat());
    let mdat = mp4_box(b"mdat", &[0u8; 5000]);
    let file = [ftyp, moov, mdat].concat();
    let d = tmpdir("mp4");
    let f = write(&d.join("synthetic.mp4"), &file);
    let s = stdout(&run(&["--Consumers=MP4", "--Reports=AVD3", "--PrintReports", "--ForwardConsoleCursorOnly", &f]));
    assert!(s.contains("MajorBrands=isom MinorVersion=512 CompatibleBrands=isom/iso2/avc1/mp41"), "{s}");
    assert!(s.contains("<Duration p=\"MP4Provider\" t=\"Double\" u=\"s\">3.5</Duration>"));
    assert!(s.contains("<Item>mp4</Item>"));
    assert!(s.contains("<VideoStream>"));
    assert!(s.contains("<PixelDimensions p=\"MP4Provider\" t=\"Dimensions\" u=\"Dimensionsless\">320, 240</PixelDimensions>"));
    assert!(s.contains("<ContainerCodecId p=\"MP4Provider\" t=\"String\" u=\"Dimensionsless\">avc1</ContainerCodecId>"));
    assert!(s.contains("<Language p=\"MP4Provider\" t=\"String\" u=\"Dimensionsless\">eng</Language>"));
    assert!(s.contains("<Duration p=\"MP4Provider\" t=\"TimeSpan\" u=\"s\">00:00:03.5000000</Duration>"));
}

// ------------------------------------------------------------------ file move

#[test]
fn file_move_with_placeholders() {
    let d = tmpdir("filemove");
    let src = d.join("original.dat");
    write(&src, b"abc");
    let log = d.join("move.log");
    let pattern = format!("{}/renamed_${{Hash-CRC32-16-UC}}${{FileExtension}}", d.join("moved").display());
    let o = bin()
        .args([
            "--Consumers=CRC32",
            "--FileMove.Mode=PlaceholderInline",
            &format!("--FileMove.Pattern={pattern}"),
            "--FileMove.Replacements=renamed=final",
            &format!("--FileMove.LogPath={}", log.display()),
            "--ForwardConsoleCursorOnly",
            src.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", stdout(&o));
    assert!(!src.exists());
    let target = d.join("moved").join("final_352441C2.dat");
    assert!(target.exists(), "{:?}", fs::read_dir(d.join("moved")).map(|r| r.count()));
    let logged = fs::read_to_string(&log).unwrap();
    assert!(logged.contains("original.dat => "));
    assert!(logged.contains("final_352441C2.dat"));
}

#[test]
fn file_move_disable_flags_and_script_file() {
    let d = tmpdir("filemove2");
    let src = d.join("keep.dat");
    write(&src, b"abc");
    let script = write(&d.join("pattern.txt"), format!("{}/elsewhere/${{FileNameWithoutExtension}}_x${{FileExtension}}\n", d.display()).as_bytes());
    let o = bin()
        .args(["--Consumers=CRC32", "--FileMove.Mode=PlaceholderFile", &format!("--FileMove.Pattern={script}"), "--FileMove.DisableFileMove", "--ForwardConsoleCursorOnly", src.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", stdout(&o));
    assert!(d.join("keep_x.dat").exists(), "renamed in place");
    assert!(!d.join("elsewhere").exists());

    let o = run(&["--Consumers=CRC32", "--FileMove.Mode=CSharpScriptInline", "--FileMove.Pattern=x", "y"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stdout(&o).contains("not supported"));
    let o = run(&["--Consumers=CRC32", "--FileMove.Mode=PlaceholderInline", "--FileMove.Pattern=x", "--FileMove.Test", "y"]);
    assert!(stdout(&o).contains("cannot enter test mode"));
}

// ------------------------------------------------------------------ errors

#[test]
fn errors_are_reported_and_saved() {
    let d = tmpdir("errors");
    let f = write(&d.join("in.dat"), b"abc");
    let errdir = d.join("errors");
    // CPY into a path that cannot be created (a file used as directory).
    let blocker = write(&d.join("blocker"), b"x");
    let o = bin()
        .args([&format!("--Consumers=CPY:{blocker}/sub"), "--SaveErrors", &format!("--ErrorDirectory={}", errdir.display()), "--IncludePersonalData", "--ForwardConsoleCursorOnly", &f])
        .output()
        .unwrap();
    let s = stdout(&o);
    assert!(s.contains("Error "), "{s}");
    let files: Vec<_> = fs::read_dir(&errdir).unwrap().map(|e| e.unwrap().path()).collect();
    assert!(!files.is_empty());
    let xml = fs::read_to_string(&files[0]).unwrap();
    assert!(xml.contains("<AVD3UIException"));
    assert!(xml.contains("<Information>"));
    assert!(xml.contains("<EffectiveCommandLineArguments>"));
    assert!(xml.contains("in.dat"));
}

#[test]
fn concurrency_partitions_process_everything() {
    let d = tmpdir("conc");
    for i in 0..12 {
        write(&d.join(format!("f{i}.bin")), &vec![i as u8; 200_000]);
    }
    let dir = d.to_string_lossy().into_owned();
    let o = bin().args(["--Consumers=CRC32,MD5", &format!("--Concurrent=4:{dir},2"), "--BufferLength=4", "--PrintHashes", "--ForwardConsoleCursorOnly", &dir]).output().unwrap();
    assert!(o.status.success(), "{}", stdout(&o));
    let s = stdout(&o);
    assert_eq!(s.matches("MD5 => ").count(), 12);
    assert!(s.contains("12/12 Files"));
}
