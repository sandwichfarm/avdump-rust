//! Cluster/block statistics: per-track sample rate histograms, bitrate, duration.

use super::ebml::EbmlReader;
use super::ids::*;
use super::sections::TrackEntrySection;
use crate::processing::ProcessingError;
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackTimecode {
    pub timecode: u64,
    pub frame_count_minus_one: u8,
    pub size: i64,
}

impl TrackTimecode {
    pub fn frame_count(&self) -> u32 {
        self.frame_count_minus_one as u32 + 1
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SampleRateCountPair {
    pub sample_rate: f64,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrackInfo {
    pub average_bitrate: Option<f64>,
    pub average_sample_rate: Option<f64>,
    pub min_sample_rate: Option<f64>,
    pub max_sample_rate: Option<f64>,
    pub sample_rate_histogram: Vec<SampleRateCountPair>,
    /// Track length in seconds.
    pub track_length: f64,
    pub track_size: i64,
    pub sample_count: i64,
}

#[derive(Debug, Clone)]
pub struct Track {
    pub track_number: i64,
    pub timecode_scale: f64,
    pub timecodes: Vec<TrackTimecode>,
    codec_id: Option<String>,
    codec_private: Option<Vec<u8>>,
}

impl Track {
    fn new(track_number: i64, mkv_track: Option<&TrackEntrySection>) -> Self {
        Self {
            track_number,
            timecode_scale: mkv_track.and_then(|t| t.track_timecode_scale).unwrap_or(1.0),
            timecodes: Vec::new(),
            codec_id: mkv_track.and_then(|t| t.codec_id.clone()),
            codec_private: mkv_track.and_then(|t| t.codec_private.clone()),
        }
    }

    pub fn track_info(&self) -> Option<TrackInfo> {
        let mut timecodes = self.timecodes.clone();

        // Hack to get info from subtitles stored in CodecPrivate.
        if timecodes.is_empty() {
            if let (Some(codec), Some(private)) = (&self.codec_id, &self.codec_private) {
                if codec == "S_TEXT/ASS" || codec == "S_TEXT/SSA" {
                    timecodes = extract_subtitle_timecodes(private);
                }
            }
        }
        timecodes.sort_by_key(|t| t.timecode);

        let first = *timecodes.first()?;
        let mut rate = [0f64; 3];
        let mut min_sample_rate: Option<f64> = None;
        let mut max_sample_rate: Option<f64> = None;
        let mut old = first;
        let (mut pos, mut prev_pos, mut prevprev_pos): (usize, usize, usize);
        pos = 0;
        prev_pos = 0;
        let mut frames: i64 = first.frame_count() as i64;
        let mut track_size: i64 = self.codec_private.as_ref().map(|p| p.len() as i64).unwrap_or(0) + first.size;

        // Keep insertion order to mirror the .NET Dictionary enumeration order.
        let mut histogram: BTreeMap<u64, (f64, i64)> = BTreeMap::new();
        let mut histogram_order: Vec<u64> = Vec::new();

        for tc in timecodes.iter().skip(1) {
            let delta = tc.timecode as f64 - old.timecode as f64;
            rate[pos] = (1_000_000_000f64 * old.frame_count() as f64) / delta;
            if rate[pos].is_finite() {
                let key = rate[pos].to_bits();
                let entry = histogram.entry(key).or_insert_with(|| {
                    histogram_order.push(key);
                    (rate[pos], 0)
                });
                entry.1 += 1;
            }
            old = *tc;
            prevprev_pos = prev_pos;
            prev_pos = pos;
            pos = (pos + 1) % 3;

            track_size += tc.size;
            frames += tc.frame_count() as i64;

            let max_diff = (rate[prevprev_pos] + rate[pos] / 2.0) * 0.1;
            if (rate[prev_pos] - rate[prevprev_pos]).abs() < max_diff && (rate[prev_pos] - rate[pos]).abs() < max_diff {
                let r = rate[prev_pos];
                if min_sample_rate.map(|m| m > r).unwrap_or(true) {
                    min_sample_rate = Some(r);
                }
                if max_sample_rate.map(|m| m < r).unwrap_or(true) {
                    max_sample_rate = Some(r);
                }
            }
        }

        let last = *timecodes.last()?;
        let track_length_ms = ((last.timecode - first.timecode) / 1_000_000) as f64;
        let track_length = track_length_ms / 1000.0;

        Some(TrackInfo {
            sample_rate_histogram: histogram_order.iter().map(|k| {
                let (rate, count) = histogram[k];
                SampleRateCountPair { sample_rate: rate, count }
            }).collect(),
            average_bitrate: if track_size != 0 && track_length_ms != 0.0 { Some(track_size as f64 * 8.0 / track_length) } else { None },
            average_sample_rate: if frames != 0 && track_length_ms != 0.0 { Some(frames as f64 / track_length) } else { None },
            min_sample_rate,
            max_sample_rate,
            track_length,
            track_size,
            sample_count: frames,
        })
    }
}

fn extract_subtitle_timecodes(codec_private: &[u8]) -> Vec<TrackTimecode> {
    let content = String::from_utf8_lossy(codec_private);
    let mut out = Vec::new();
    let events = match content.find("[Events]") {
        Some(p) => p,
        None => return out,
    };
    let format_start = match content[events..].find("Format:") {
        Some(p) => events + p + 7,
        None => return out,
    };
    let format_end = match content[format_start..].find('\n') {
        Some(p) => format_start + p,
        None => return out,
    };
    let columns: Vec<String> = content[format_start..format_end].replace(' ', "").trim_end_matches('\r').split(',').map(|s| s.to_string()).collect();
    let start_index = match columns.iter().position(|c| c == "Start") {
        Some(i) => i,
        None => return out,
    };
    for line in content[format_end + 1..].split(['\n', '\r']).filter(|l| !l.is_empty()) {
        let parts: Vec<&str> = line.splitn(columns.len(), ',').collect();
        if let Some(start) = parts.get(start_index) {
            if let Some(ms) = parse_ass_time(start) {
                out.push(TrackTimecode { timecode: (ms * 1_000_000.0) as u64, frame_count_minus_one: 0, size: 0 });
            }
        }
    }
    out
}

/// Parse `h:mm:ss.ff` into milliseconds.
fn parse_ass_time(s: &str) -> Option<f64> {
    let s = s.trim();
    let mut parts = s.split(':');
    let h: f64 = parts.next()?.trim().parse().ok()?;
    let m: f64 = parts.next()?.trim().parse().ok()?;
    let sec: f64 = parts.next()?.trim().parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(((h * 60.0 + m) * 60.0 + sec) * 1000.0)
}

#[derive(Debug, Clone, Default)]
pub struct ClusterSection {
    pub tracks: HashMap<i64, Track>,
    pub timecode_scale: u64,
    timecode: i64,
}

impl ClusterSection {
    pub fn add_tracks(&mut self, tracks: &[TrackEntrySection]) {
        for track in tracks {
            let number = match track.track_number {
                Some(n) => n as i64,
                None => continue,
            };
            if let Some(existing) = self.tracks.remove(&!number) {
                let mut replace = Track::new(number, Some(track));
                replace.timecodes.extend(existing.timecodes);
                self.tracks.insert(number, replace);
            } else {
                self.tracks.insert(number, Track::new(number, Some(track)));
            }
        }
    }

    /// Read the children of the current Cluster (or BlockGroup) element.
    pub fn read(&mut self, reader: &mut EbmlReader<'_, '_>) -> Result<(), ProcessingError> {
        reader.enter();
        let result = (|| -> Result<(), ProcessingError> {
            while let Some(h) = reader.next()? {
                match h.id {
                    SIMPLE_BLOCK | BLOCK => {
                        let block = match reader.read_block_header() {
                            Ok(b) => b,
                            Err(_) => continue,
                        };
                        let tn = block.track_number as i64;
                        let track = if self.tracks.contains_key(&tn) {
                            self.tracks.get_mut(&tn).unwrap()
                        } else if self.tracks.contains_key(&!tn) {
                            self.tracks.get_mut(&!tn).unwrap()
                        } else {
                            self.tracks.entry(!tn).or_insert_with(|| Track::new(!tn, None))
                        };
                        let scaled = (block.timecode as i64 + self.timecode) as f64 * track.timecode_scale * self.timecode_scale as f64;
                        let data_len = h.size.unwrap_or(0) as i64;
                        track.timecodes.push(TrackTimecode {
                            timecode: scaled.max(0.0) as u64,
                            frame_count_minus_one: block.frame_count_minus_one,
                            size: data_len - block.header_length as i64,
                        });
                    }
                    BLOCK_GROUP => self.read(reader)?,
                    TIMECODE => self.timecode = reader.read_uint()? as i64,
                    _ => {}
                }
            }
            Ok(())
        })();
        reader.leave()?;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ass_time() {
        assert_eq!(parse_ass_time("0:00:01.50"), Some(1500.0));
        assert_eq!(parse_ass_time("1:02:03.00"), Some(3_723_000.0));
        assert_eq!(parse_ass_time("bogus"), None);
    }

    #[test]
    fn track_info_constant_rate() {
        let mut t = Track::new(1, None);
        for i in 0..100u64 {
            t.timecodes.push(TrackTimecode { timecode: i * 40_000_000, frame_count_minus_one: 0, size: 1000 });
        }
        let info = t.track_info().unwrap();
        assert_eq!(info.sample_count, 100);
        assert_eq!(info.track_size, 100_000);
        assert!((info.track_length - 3.96).abs() < 1e-9);
        assert_eq!(info.sample_rate_histogram.len(), 1);
        assert!((info.sample_rate_histogram[0].sample_rate - 25.0).abs() < 1e-9);
        assert_eq!(info.min_sample_rate, Some(25.0));
        assert_eq!(info.max_sample_rate, Some(25.0));
    }
}
