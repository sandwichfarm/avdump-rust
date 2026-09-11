//! Converts a parsed MP4 box tree into the generic metadata tree.

use crate::info::meta::{container_types as ct, keys, MetaInfoContainer, MetaProvider};
use crate::info::value::*;
use crate::processing::consumers::mp4::*;

pub struct Mp4Provider;

impl Mp4Provider {
    pub const NAME: &'static str = "MP4Provider";

    pub fn create(root: Option<&Mp4Node>) -> MetaProvider {
        let mut p = MetaProvider::new(Self::NAME, ct::MEDIA_PROVIDER);
        let root = match root {
            Some(r) => r,
            None => return p,
        };
        let d = keys::DIMENSIONLESS;
        p.add("FileSize", keys::BYTES, Value::I64(root.size as i64));
        if let Some(ftyp) = root.first_descendant(b"ftyp").and_then(|n| FileTypeBox::parse(&n.data)) {
            p.add("ContainerVersion", d, Value::Str(format!("MajorBrands={} MinorVersion={} CompatibleBrands={}", ftyp.major_brand, ftyp.minor_version, ftyp.compatible_brands.join("/"))));
        }
        if let Some(mvhd) = root.first_descendant(b"mvhd").and_then(|n| MovieHeaderBox::parse(&n.data)) {
            p.add_opt("CreationDate", d, mvhd.creation_date.map(Value::DateTime));
            p.add_opt("ModificationDate", d, mvhd.modification_date.map(Value::DateTime));
            if mvhd.timescale != 0 {
                p.add("Duration", keys::SECONDS, Value::F64(mvhd.duration as f64 / mvhd.timescale as f64));
            }
        }

        let tracks = root.descendants(b"trak");
        let handler_of = |t: &Mp4Node| t.first_descendant(b"hdlr").and_then(|n| HandlerBox::parse(&n.data)).map(|h| h.handler_type);
        let all_audio = !tracks.is_empty() && tracks.iter().all(|t| handler_of(t).as_deref() == Some("soun"));
        p.add("SuggestedFileExtension", d, Value::Strings(vec![if all_audio { "m4a" } else { "mp4" }.to_string()]));

        for track in tracks {
            Self::populate_track(&mut p, track);
        }
        p
    }

    fn populate_track(p: &mut MetaProvider, track: &Mp4Node) {
        let d = keys::DIMENSIONLESS;
        let tkhd = match track.first_descendant(b"tkhd").and_then(|n| TrackHeaderBox::parse(&n.data)) {
            Some(t) => t,
            None => return,
        };
        let mdhd = track.first_descendant(b"mdhd").and_then(|n| MediaHeaderBox::parse(&n.data));
        let stsd = track.first_descendant(b"stsd").and_then(|n| SampleDescriptionBox::parse(&n.data));
        let handler = track.first_descendant(b"hdlr").and_then(|n| HandlerBox::parse(&n.data)).map(|h| h.handler_type).unwrap_or_default();

        let mut stream = match handler.as_str() {
            "vide" => {
                let mut s = MetaInfoContainer::new(tkhd.track_id as u64, ct::VIDEO_STREAM);
                let mut dims = Some(Dimensions { width: tkhd.width as i32, height: tkhd.height as i32 });
                if let Some(stsd) = &stsd {
                    for i in 0..stsd.entry_count() {
                        if let Some(entry) = stsd.video_entry(i) {
                            let dd = dims.unwrap();
                            if entry.width as i32 != dd.width || entry.height as i32 != dd.height {
                                dims = None;
                                break;
                            }
                        }
                    }
                }
                p.add_to(&mut s, "PixelDimensions", d, dims.map(Value::Dimensions));
                p.add_to(&mut s, "DisplayDimensions", d, dims.map(Value::Dimensions));
                if let Some(stsd) = &stsd {
                    if let Some(entry) = stsd.video_entry(0) {
                        p.add_to(&mut s, "ContainerCodecId", d, Some(Value::Str(entry.format.clone())));
                    }
                }
                s
            }
            "soun" => {
                let mut s = MetaInfoContainer::new(tkhd.track_id as u64, ct::AUDIO_STREAM);
                if let Some(stsd) = &stsd {
                    if let Some((format, _)) = stsd.entries.first() {
                        p.add_to(&mut s, "ContainerCodecId", d, Some(Value::Str(format.clone())));
                    }
                }
                s
            }
            "hint" => return,
            _ => MetaInfoContainer::new(tkhd.track_id as u64, ct::MEDIA_STREAM),
        };

        p.add_to(&mut stream, "Id", d, Some(Value::U64(tkhd.track_id as u64)));
        let note = |src: &str| vec![("Source".to_string(), src.to_string())];
        p.add_to_with_notes(&mut stream, "CreationDate", d, tkhd.creation_date.map(Value::DateTime), note("TrackHeader"));
        p.add_to_with_notes(&mut stream, "ModificationDate", d, tkhd.modification_date.map(Value::DateTime), note("TrackHeader"));
        if let Some(mdhd) = &mdhd {
            p.add_to_with_notes(&mut stream, "CreationDate", d, mdhd.creation_date.map(Value::DateTime), note("MediaHeader"));
            p.add_to_with_notes(&mut stream, "ModificationDate", d, mdhd.modification_date.map(Value::DateTime), note("MediaHeader"));
            if mdhd.timescale != 0 {
                p.add_to(&mut stream, "Duration", keys::SECONDS, Some(Value::TimeSpan(mdhd.duration as f64 / mdhd.timescale as f64)));
            }
            if !mdhd.language.is_empty() && mdhd.language != "```" {
                p.add_to(&mut stream, "Language", d, Some(Value::Str(mdhd.language.clone())));
            }
        }
        p.add_node(stream);
    }
}
