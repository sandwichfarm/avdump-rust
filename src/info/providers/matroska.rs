//! Converts a parsed [`MatroskaFile`] into the generic metadata tree.

use crate::info::meta::{container_types as ct, keys, MetaInfoContainer, MetaProvider};
use crate::info::value::*;
use crate::processing::consumers::matroska::cluster::SampleRateCountPair;
use crate::processing::consumers::matroska::sections::{chapter_flags, edition_flags, track_flags, ArType, ChapterAtomSection, EditionEntrySection, SimpleTagSection, StereoMode, TagsSection, TrackEntrySection, TrackType};
use crate::processing::consumers::matroska::sections::DisplayUnit as MkvDisplayUnit;
use crate::processing::consumers::matroska::MatroskaFile;

pub struct MatroskaProvider;

impl MatroskaProvider {
    pub const NAME: &'static str = "MatroskaProvider";

    pub fn create(mfi: Option<&MatroskaFile>) -> MetaProvider {
        let mut p = MetaProvider::new(Self::NAME, ct::MEDIA_PROVIDER);
        let mfi = match mfi {
            Some(m) if m.has_meta_data() && m.segment.is_some() => m,
            _ => return p,
        };
        let segment = mfi.segment.as_ref().unwrap();
        let info = segment.segment_info.as_ref();
        let tracks = segment.tracks.as_ref();

        p.add("FileSize", keys::BYTES, Value::I64(mfi.section_size as i64));
        p.add("ContainerVersion", keys::DIMENSIONLESS, Value::Str(format!("DocType={} DocTypeVersion={}", mfi.ebml_header.doc_type(), mfi.ebml_header.doc_type_version())));
        if let Some(info) = info {
            p.add_opt("CreationDate", keys::DIMENSIONLESS, info.production_date.map(Value::DateTime));
            p.add_opt("Duration", keys::SECONDS, info.duration.map(|d| Value::F64(d * info.timecode_scale() as f64 / 1_000_000_000.0)));
            p.add_opt("Id", keys::DIMENSIONLESS, info.segment_uid.clone().map(Value::Binary));
            p.add_opt("PreviousId", keys::DIMENSIONLESS, info.previous_uid.clone().map(Value::Binary));
            p.add_opt("NextId", keys::DIMENSIONLESS, info.next_uid.clone().map(Value::Binary));
            p.add_opt("PreviousName", keys::DIMENSIONLESS, info.previous_filename.clone().map(Value::Str));
            p.add_opt("NextFileName", keys::DIMENSIONLESS, info.next_filename.clone().map(Value::Str));
            p.add_opt("WritingApp", keys::DIMENSIONLESS, info.writing_app.clone().map(Value::Str));
            p.add_opt("MuxingApp", keys::DIMENSIONLESS, info.muxing_app.clone().map(Value::Str));
        }

        let track_items: &[TrackEntrySection] = tracks.map(|t| t.items.as_slice()).unwrap_or(&[]);
        let has = |t: TrackType| track_items.iter().any(|e| e.track_type() == t);
        let ext = if mfi.ebml_header.doc_type().eq_ignore_ascii_case("webm") {
            Some("webm")
        } else if track_items.iter().any(|e| e.track_type() == TrackType::Video && e.video.as_ref().map(|v| v.stereo_mode() != StereoMode::Mono).unwrap_or(false)) {
            Some("mk3d")
        } else if has(TrackType::Video) {
            Some("mkv")
        } else if has(TrackType::Audio) {
            Some("mka")
        } else if has(TrackType::Subtitle) {
            Some("mks")
        } else {
            None
        };
        if let Some(e) = ext {
            p.add("SuggestedFileExtension", keys::DIMENSIONLESS, Value::Strings(vec![e.to_string()]));
        }

        let mut indices = [0i32; 4];
        for track in track_items {
            Self::populate_track(&mut p, mfi, track, &mut indices);
        }

        if let Some(tags) = segment.tags.iter().max_by_key(|t| t.items.len()) {
            Self::populate_tags(&mut p, tags);
        }

        p.add("AttachmentSize", keys::BYTES, Value::I64(segment.attachments.as_ref().and_then(|a| a.section_size).unwrap_or(0) as i64));
        if let Some(att) = &segment.attachments {
            for a in &att.items {
                let mut node = MetaInfoContainer::new(a.file_uid, ct::ATTACHMENT);
                p.add_to(&mut node, "Id", keys::DIMENSIONLESS, Some(Value::I64(a.file_uid as i64)));
                p.add_to(&mut node, "Size", keys::BYTES, a.section_size.map(|s| Value::I64(s as i64)));
                p.add_to(&mut node, "Type", keys::DIMENSIONLESS, a.file_mime_type.clone().map(Value::Str));
                p.add_to(&mut node, "Description", keys::DIMENSIONLESS, a.file_description.clone().map(Value::Str));
                p.add_node(node);
            }
        }

        if let Some(chapters) = &segment.chapters {
            for edition in chapters.items.iter().filter(|e| !e.chapter_atoms.is_empty()) {
                Self::populate_chapters(&mut p, edition);
            }
        }
        p
    }

    fn populate_chapters(p: &mut MetaProvider, edition: &EditionEntrySection) {
        let id = edition.edition_uid.unwrap_or(p.root.count_nodes(ct::CHAPTERS) as u64);
        let mut chapters = MetaInfoContainer::new(id, ct::CHAPTERS);
        p.add_to(&mut chapters, "Id", keys::DIMENSIONLESS, edition.edition_uid.map(|v| Value::I32(v as i32)));
        let flags = edition.edition_flags();
        p.add_to(&mut chapters, "IsHidden", keys::DIMENSIONLESS, Some(Value::Bool(flags & edition_flags::HIDDEN != 0)));
        p.add_to(&mut chapters, "IsDefault", keys::DIMENSIONLESS, Some(Value::Bool(flags & edition_flags::DEFAULT != 0)));
        p.add_to(&mut chapters, "IsOrdered", keys::DIMENSIONLESS, Some(Value::Bool(flags & edition_flags::ORDERED != 0)));
        for atom in &edition.chapter_atoms {
            Self::populate_chapter(p, atom, &mut chapters);
        }
        p.add_node(chapters);
    }

    fn populate_chapter(p: &MetaProvider, atom: &ChapterAtomSection, parent: &mut MetaInfoContainer) {
        let id = atom.chapter_uid.unwrap_or(parent.count_nodes(ct::CHAPTER) as u64);
        let mut chapter = MetaInfoContainer::new(id, ct::CHAPTER);
        p.add_to(&mut chapter, "Id", keys::DIMENSIONLESS, atom.chapter_uid.map(|v| Value::I32(v as i32)));
        p.add_to(&mut chapter, "IdString", keys::DIMENSIONLESS, atom.chapter_string_uid.clone().map(Value::Str));
        p.add_to(&mut chapter, "TimeStart", "byte", atom.chapter_time_start.map(|v| Value::F64(v as f64 / 1_000_000_000.0)));
        // The original declares TimeEnd with the key "TimeStart" as well, so the end time never
        // surfaces; we expose it under its intended name.
        p.add_to(&mut chapter, "TimeEnd", "byte", atom.chapter_time_end.map(|v| Value::F64(v as f64 / 1_000_000_000.0)));
        let flags = atom.chapter_flags();
        p.add_to(&mut chapter, "IsHidden", keys::DIMENSIONLESS, Some(Value::Bool(flags & chapter_flags::HIDDEN != 0)));
        p.add_to(&mut chapter, "IsEnabled", keys::DIMENSIONLESS, Some(Value::Bool(flags & chapter_flags::ENABLED != 0)));
        p.add_to(&mut chapter, "SegmentId", keys::DIMENSIONLESS, atom.chapter_segment_uid.clone().map(Value::Binary));
        p.add_to(&mut chapter, "SegmentChaptersId", keys::DIMENSIONLESS, atom.chapter_segment_edition_uid.map(|v| Value::I32(v as i32)));
        p.add_to(&mut chapter, "PhysicalEquivalent", keys::DIMENSIONLESS, atom.chapter_physical_equiv.map(|v| Value::I32(v as i32)));
        if let Some(t) = &atom.chapter_track {
            for tid in &t.chapter_track_numbers {
                p.add_to(&mut chapter, "AssociatedTrack", keys::DIMENSIONLESS, Some(Value::I32(*tid as i32)));
            }
        }
        let titles: Vec<ChapterTitle> = atom
            .chapter_displays
            .iter()
            .map(|d| ChapterTitle { title: d.chapter_string.clone().unwrap_or_default(), languages: d.chapter_languages.clone(), countries: d.chapter_countries.clone() })
            .collect();
        p.add_to(&mut chapter, "Titles", keys::DIMENSIONLESS, Some(Value::ChapterTitles(titles)));
        for sub in &atom.chapter_atoms {
            Self::populate_chapter(p, sub, &mut chapter);
        }
        p.add_to(&mut chapter, "HasOperations", keys::DIMENSIONLESS, Some(Value::Bool(!atom.chapter_processes.is_empty())));
        parent.add_node(chapter);
    }

    fn populate_tags(p: &mut MetaProvider, mkv_tags: &TagsSection) {
        let mut tags = Vec::new();
        for tag in &mkv_tags.items {
            let mut targets = Vec::new();
            targets.extend(tag.targets.track_uids().into_iter().map(|id| TagTarget { kind: TagTargetKind::Track, id: id as i64 }));
            targets.extend(tag.targets.edition_uids().into_iter().map(|id| TagTarget { kind: TagTargetKind::Chapter, id: id as i64 }));
            targets.extend(tag.targets.chapter_uids().into_iter().map(|id| TagTarget { kind: TagTargetKind::Chapters, id: id as i64 }));
            targets.extend(tag.targets.attachment_uids().into_iter().map(|id| TagTarget { kind: TagTargetKind::Attachment, id: id as i64 }));
            tags.push(TargetedTag { targets, tags: tag.simple_tags.iter().map(Self::convert_tag).collect() });
        }
        p.add("Tags", keys::DIMENSIONLESS, Value::Tags(tags));
    }

    fn convert_tag(tag: &SimpleTagSection) -> Tag {
        Tag {
            name: tag.tag_name.clone().unwrap_or_default(),
            value: match &tag.tag_string {
                Some(s) => TagValue::Text(s.clone()),
                None => TagValue::Binary(tag.tag_binary().to_vec()),
            },
            language: tag.tag_language().to_string(),
            is_default: tag.tag_default(),
            children: tag.simple_tags.iter().map(Self::convert_tag).collect(),
        }
    }

    fn populate_track(p: &mut MetaProvider, mfi: &MatroskaFile, track: &TrackEntrySection, indices: &mut [i32; 4]) {
        let segment = mfi.segment.as_ref().unwrap();
        let track_info = track.track_number.and_then(|n| segment.cluster.tracks.get(&(n as i64))).and_then(|t| t.track_info());

        let (mut stream, track_index) = match track.track_type() {
            TrackType::Video => {
                let mut s = MetaInfoContainer::new(track.track_uid.unwrap_or(p.root.count_nodes(ct::VIDEO_STREAM) as u64), ct::VIDEO_STREAM);
                let idx = indices[0];
                indices[0] += 1;
                if let Some(v) = &track.video {
                    let d = keys::DIMENSIONLESS;
                    p.add_to(&mut s, "AspectRatioBehavior", d, Some(Value::AspectRatioBehavior(match v.aspect_ratio_type() {
                        ArType::FreeResizing => AspectRatioBehavior::FreeResizing,
                        ArType::KeepAr => AspectRatioBehavior::KeepAR,
                        ArType::Fixed => AspectRatioBehavior::Fixed,
                        ArType::Unknown => AspectRatioBehavior::Unknown,
                    })));
                    p.add_to(&mut s, "ColorSpace", d, v.color_space.as_ref().filter(|c| c.len() >= 4).map(|c| Value::I32(i32::from_le_bytes([c[0], c[1], c[2], c[3]]))));
                    p.add_to(&mut s, "DisplayDimensions", d, Some(Value::Dimensions(Dimensions { width: v.display_width() as i32, height: v.display_height() as i32 })));
                    p.add_to(&mut s, "DisplayUnit", d, Some(Value::DisplayUnit(match v.display_unit() {
                        MkvDisplayUnit::Pixels => DisplayUnit::Pixel,
                        MkvDisplayUnit::Centimeters => DisplayUnit::Meter,
                        MkvDisplayUnit::Inches => DisplayUnit::Meter,
                        MkvDisplayUnit::AspectRatio => DisplayUnit::AspectRatio,
                        _ => DisplayUnit::Unknown,
                    })));
                    p.add_to(&mut s, "HasAlpha", d, Some(Value::Bool(v.alpha_mode() != 0)));
                    p.add_to(&mut s, "IsInterlaced", d, Some(Value::Bool(v.interlaced())));
                    p.add_to(&mut s, "PixelCrop", d, Some(Value::CropSides(CropSides { top: v.pixel_crop_top() as i32, right: v.pixel_crop_right() as i32, bottom: v.pixel_crop_bottom() as i32, left: v.pixel_crop_left() as i32 })));
                    p.add_to(&mut s, "PixelDimensions", d, Some(Value::Dimensions(Dimensions { width: v.pixel_width as i32, height: v.pixel_height as i32 })));
                    p.add_to(&mut s, "StorageAspectRatio", d, Some(Value::F64(v.pixel_width as f64 / v.pixel_height as f64)));
                    p.add_to(&mut s, "PixelAspectRatio", d, Some(Value::F64((v.display_width() * v.pixel_height) as f64 / (v.display_height() * v.pixel_width) as f64)));
                    p.add_to(&mut s, "DisplayAspectRatio", d, Some(Value::F64(v.display_width() as f64 / v.display_height() as f64)));
                    p.add_to(&mut s, "SampleCount", d, track_info.as_ref().map(|t| Value::I64(t.sample_count)));
                    p.add_to(&mut s, "StatedSampleRate", keys::HZ, Some(Value::F64(track.default_duration.map(|dd| 1_000_000_000.0 / dd as f64).unwrap_or(0.0))));
                    p.add_to(&mut s, "StereoMode", d, Some(Value::StereoModes(convert_stereo(v.stereo_mode()))));
                }
                (s, idx)
            }
            TrackType::Audio => {
                let mut s = MetaInfoContainer::new(track.track_uid.unwrap_or(p.root.count_nodes(ct::AUDIO_STREAM) as u64), ct::AUDIO_STREAM);
                let idx = indices[1];
                indices[1] += 1;
                if let Some(a) = &track.audio {
                    p.add_to(&mut s, "BitDepth", keys::DIMENSIONLESS, a.bit_depth.map(|b| Value::I32(b as i32)));
                    p.add_to(&mut s, "ChannelCount", keys::DIMENSIONLESS, Some(Value::I32(a.channel_count() as i32)));
                    p.add_to(&mut s, "OutputSampleRate", keys::HZ, Some(Value::F64(a.output_sampling_frequency())));
                    p.add_to(&mut s, "StatedSampleRate", keys::HZ, Some(Value::F64(a.sampling_frequency())));
                    if let Some(ti) = &track_info {
                        let ticks = ti.track_length * 10_000_000.0;
                        p.add_to(&mut s, "SampleCount", keys::DIMENSIONLESS, Some(Value::I64(((ticks * a.sampling_frequency()) as i64) / 10_000_000)));
                    }
                }
                (s, idx)
            }
            TrackType::Subtitle => {
                let s = MetaInfoContainer::new(track.track_uid.unwrap_or(p.root.count_nodes(ct::SUBTITLE_STREAM) as u64), ct::SUBTITLE_STREAM);
                let idx = indices[2];
                indices[2] += 1;
                (s, idx)
            }
            _ => {
                let s = MetaInfoContainer::new(track.track_uid.unwrap_or(p.root.count_nodes(ct::MEDIA_STREAM) as u64), ct::MEDIA_STREAM);
                let idx = indices[3];
                indices[3] += 1;
                (s, idx)
            }
        };

        if let Some(ti) = &track_info {
            if matches!(track.track_type(), TrackType::Video | TrackType::Audio) {
                p.add_to(&mut stream, "SampleRateHistogram", keys::HZ, Some(Value::SampleRateHistogram(ti.sample_rate_histogram.iter().map(|x| (x.sample_rate, x.count)).collect())));
                p.add_to(&mut stream, "AverageSampleRate", keys::HZ, ti.average_sample_rate.map(Value::F64));
                p.add_to(&mut stream, "MinSampleRate", keys::HZ, ti.min_sample_rate.map(Value::F64));
                p.add_to(&mut stream, "MaxSampleRate", keys::HZ, ti.max_sample_rate.map(Value::F64));
                let dominant = ti.sample_rate_histogram.iter().max_by_key(|x| x.count).map(|x| x.sample_rate);
                p.add_to(&mut stream, "DominantSampleRate", keys::HZ, dominant.map(Value::F64));
                p.add_to(&mut stream, "SampleRateVariance", keys::HZ, Some(Value::F64(calc_deviation(&ti.sample_rate_histogram))));
                p.add_to(&mut stream, "Bitrate", keys::BITS_PER_SECOND, ti.average_bitrate.map(Value::F64));
            }
        }

        let d = keys::DIMENSIONLESS;
        let flags = track.track_flags();
        p.add_to(&mut stream, "Index", d, Some(Value::I32(track_index)));
        p.add_to(&mut stream, "Id", d, track.track_uid.map(Value::U64));
        p.add_to(&mut stream, "IsDefault", d, Some(Value::Bool(flags & track_flags::DEFAULT != 0)));
        p.add_to(&mut stream, "IsEnabled", d, Some(Value::Bool(flags & track_flags::ENABLED != 0)));
        p.add_to(&mut stream, "IsForced", d, Some(Value::Bool(flags & track_flags::FORCED != 0)));
        p.add_to(&mut stream, "IsOverlay", d, Some(Value::Bool(!track.track_overlay.is_empty())));
        p.add_to(&mut stream, "Language", d, Some(Value::Str(track.language().to_string())));
        p.add_to(&mut stream, "Title", d, track.name.clone().map(Value::Str));
        p.add_to(&mut stream, "ContainerCodecId", d, track.codec_id.clone().map(Value::Str));
        p.add_to(&mut stream, "ContainerCodecName", d, track.codec_name.clone().map(Value::Str));
        if let Some(cues) = &segment.cues {
            let count = cues.cue_points.iter().filter(|cp| cp.cue_track_positions.iter().any(|tp| Some(tp.cue_track) == track.track_number)).count();
            p.add_to(&mut stream, "CueCount", d, Some(Value::I32(count as i32)));
        }
        if let Some(cp) = &track.codec_private {
            p.add_to(&mut stream, "CodecPrivateSize", keys::BYTES, Some(Value::I32(cp.len() as i32)));
            if track.codec_id.as_deref() == Some("V_MS/VFW/FOURCC") && cp.len() >= 40 {
                let compression = &cp[16..20];
                let fourcc: String = compression.iter().map(|b| *b as char).collect();
                p.add_to(&mut stream, "ContainerCodecCC", d, Some(Value::Str(fourcc)));
            }
            if track.codec_id.as_deref() == Some("A_MS/ACM") && cp.len() >= 18 {
                let tag = u16::from_le_bytes([cp[0], cp[1]]);
                p.add_to(&mut stream, "ContainerCodecCC", d, Some(Value::Str(format!("{:x}", tag))));
            }
        }
        if let Some(ti) = &track_info {
            p.add_to(&mut stream, "Duration", keys::SECONDS, Some(Value::TimeSpan(ti.track_length)));
            p.add_to(&mut stream, "Size", keys::BYTES, Some(Value::I64(ti.track_size)));
        }
        p.add_node(stream);
    }
}

fn calc_deviation(histogram: &[SampleRateCountPair]) -> f64 {
    let count: i64 = histogram.iter().map(|i| i.count).sum();
    let sqr_sum: f64 = histogram.iter().map(|i| i.sample_rate * i.sample_rate).sum();
    let mean: f64 = histogram.iter().map(|i| i.sample_rate).sum::<f64>() / count as f64;
    (sqr_sum / count as f64 - mean * mean).sqrt()
}

fn convert_stereo(s: StereoMode) -> u32 {
    use stereo_modes::*;
    match s {
        StereoMode::Mono => MONO,
        StereoMode::LeftRight => LEFT_RIGHT,
        StereoMode::BottomTop => TOP_BOTTOM | REVERSED,
        StereoMode::TopBottom => TOP_BOTTOM,
        StereoMode::CheckBoardRight => CHECKBOARD | REVERSED,
        StereoMode::CheckboardLeft => CHECKBOARD,
        StereoMode::RowInterleavedRight => ROW_INTERLEAVED | REVERSED,
        StereoMode::RowInterleavedLeft => ROW_INTERLEAVED,
        StereoMode::ColumnInterleavedRight => COLUMN_INTERLEAVED | REVERSED,
        StereoMode::ColumnInterleavedLeft => COLUMN_INTERLEAVED,
        StereoMode::AnaGlyphCyanRed => ANAGLYPH | CYAN_RED,
        StereoMode::RightLeft => LEFT_RIGHT | REVERSED,
        StereoMode::AnaGlyphGreenMagenta => ANAGLYPH | GREEN_MAGENTA,
        StereoMode::AlternatingFramesRight => FRAME_ALTERNATING | REVERSED,
        StereoMode::AlternatingFramesLeft => FRAME_ALTERNATING,
        StereoMode::Other => OTHER,
    }
}
