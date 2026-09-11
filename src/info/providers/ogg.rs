//! Converts a parsed Ogg file into the generic metadata tree.

use crate::info::meta::{container_types as ct, keys, MetaInfoContainer, MetaProvider};
use crate::info::value::*;
use crate::processing::consumers::ogg::{BitStreamKind, OggFile};

pub struct OggProvider;

impl OggProvider {
    pub const NAME: &'static str = "OggProvider";

    pub fn create(ogg: Option<&OggFile>) -> MetaProvider {
        let mut p = MetaProvider::new(Self::NAME, ct::MEDIA_PROVIDER);
        let ogg = match ogg {
            Some(o) => o,
            None => return p,
        };
        p.add("FileSize", keys::BYTES, Value::I64(ogg.file_size));
        p.add("Overhead", keys::BYTES, Value::I64(ogg.overhead));

        let mut indices = [0i32; 4];
        let d = keys::DIMENSIONLESS;
        for bs in ogg.bitstreams() {
            let mut stream;
            let track_index;
            match bs.kind {
                BitStreamKind::Video => {
                    track_index = indices[0];
                    indices[0] += 1;
                    stream = MetaInfoContainer::new(bs.id as u64, ct::VIDEO_STREAM);
                    if let Some(cc) = &bs.actual_codec_name {
                        p.add_to(&mut stream, "ContainerCodecCC", d, Some(Value::Str(cc.clone())));
                    }
                    p.add_to(&mut stream, "PixelDimensions", d, Some(Value::Dimensions(Dimensions { width: bs.width, height: bs.height })));
                    p.add_to(&mut stream, "StatedSampleRate", keys::HZ, Some(Value::F64(bs.frame_rate)));
                    p.add_to(&mut stream, "SampleCount", d, Some(Value::I64(bs.frame_count())));
                    p.add_to(&mut stream, "Duration", keys::SECONDS, Some(Value::TimeSpan(bs.duration())));
                    p.add_to(&mut stream, "Bitrate", keys::BITS_PER_SECOND, Some(Value::F64(bs.size as f64 * 8.0 / bs.duration())));
                }
                BitStreamKind::Audio => {
                    track_index = indices[1];
                    indices[1] += 1;
                    stream = MetaInfoContainer::new(bs.id as u64, ct::AUDIO_STREAM);
                    p.add_to(&mut stream, "ChannelCount", d, Some(Value::I32(bs.channel_count)));
                    p.add_to(&mut stream, "StatedSampleRate", keys::HZ, Some(Value::F64(bs.sample_rate)));
                    p.add_to(&mut stream, "SampleCount", d, Some(Value::I64(bs.sample_count())));
                    p.add_to(&mut stream, "Duration", keys::SECONDS, Some(Value::TimeSpan(bs.duration())));
                    p.add_to(&mut stream, "Bitrate", keys::BITS_PER_SECOND, Some(Value::F64(bs.size as f64 * 8.0 / bs.duration())));
                }
                BitStreamKind::Subtitle => {
                    track_index = indices[2];
                    indices[2] += 1;
                    stream = MetaInfoContainer::new(bs.id as u64, ct::SUBTITLE_STREAM);
                }
                BitStreamKind::Unknown => {
                    track_index = indices[3];
                    indices[3] += 1;
                    stream = MetaInfoContainer::new(bs.id as u64, ct::MEDIA_STREAM);
                }
            }
            p.add_to(&mut stream, "Index", d, Some(Value::I32(track_index)));
            p.add_to(&mut stream, "Id", d, Some(Value::U64(bs.id as u64)));
            p.add_to(&mut stream, "Size", keys::BYTES, Some(Value::I64(bs.size)));
            if bs.is_officially_supported {
                p.add_to(&mut stream, "ContainerCodecId", d, Some(Value::Str(bs.codec_name.clone())));
            }
            if bs.has_comments() {
                if let Some(c) = bs.comments() {
                    if let Some(v) = c.get("title") {
                        p.add_to(&mut stream, "Title", d, Some(Value::Str(v.join(","))));
                    }
                    if let Some(v) = c.get("language") {
                        p.add_to(&mut stream, "Language", d, Some(Value::Str(v.join(","))));
                    }
                }
            }
            if let Some(cc) = &bs.actual_codec_name {
                p.add_to(&mut stream, "ContainerCodecCC", d, Some(Value::Str(cc.clone())));
            }
            p.add_node(stream);
        }

        let streams = ogg.bitstreams();
        let ext = if !streams.is_empty() && streams.iter().all(|b| b.kind == BitStreamKind::Audio) {
            Some("ogg")
        } else if !streams.is_empty() && streams.iter().all(|b| b.is_officially_supported) {
            Some("ogv")
        } else if streams.iter().any(|b| b.actual_codec_name.is_some()) {
            Some("ogm")
        } else {
            None
        };
        if let Some(e) = ext {
            p.add("SuggestedFileExtension", d, Value::Strings(vec![e.to_string()]));
        }
        p
    }
}
