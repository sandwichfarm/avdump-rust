//! EBML / Matroska element IDs (with marker bits, as they appear in the file).

#![allow(dead_code)]

// EBML header
pub const EBML: u32 = 0x1A45DFA3;
pub const EBML_VERSION: u32 = 0x4286;
pub const EBML_READ_VERSION: u32 = 0x42F7;
pub const EBML_MAX_ID_LENGTH: u32 = 0x42F2;
pub const EBML_MAX_SIZE_LENGTH: u32 = 0x42F3;
pub const DOC_TYPE: u32 = 0x4282;
pub const DOC_TYPE_VERSION: u32 = 0x4287;
pub const DOC_TYPE_READ_VERSION: u32 = 0x4285;
pub const VOID: u32 = 0xEC;
pub const CRC32: u32 = 0xBF;

// Segment
pub const SEGMENT: u32 = 0x18538067;
pub const SEEK_HEAD: u32 = 0x114D9B74;
pub const SEEK: u32 = 0x4DBB;
pub const SEEK_ID: u32 = 0x53AB;
pub const SEEK_POSITION: u32 = 0x53AC;

pub const INFO: u32 = 0x1549A966;
pub const SEGMENT_UID: u32 = 0x73A4;
pub const SEGMENT_FILENAME: u32 = 0x7384;
pub const PREV_UID: u32 = 0x3CB923;
pub const PREV_FILENAME: u32 = 0x3C83AB;
pub const NEXT_UID: u32 = 0x3EB923;
pub const NEXT_FILENAME: u32 = 0x3E83BB;
pub const SEGMENT_FAMILY: u32 = 0x4444;
pub const CHAPTER_TRANSLATE: u32 = 0x6924;
pub const CHAPTER_TRANSLATE_EDITION_UID: u32 = 0x69FC;
pub const CHAPTER_TRANSLATE_CODEC: u32 = 0x69BF;
pub const CHAPTER_TRANSLATE_ID: u32 = 0x69A5;
pub const TIMECODE_SCALE: u32 = 0x2AD7B1;
pub const DURATION: u32 = 0x4489;
pub const DATE_UTC: u32 = 0x4461;
pub const TITLE: u32 = 0x7BA9;
pub const MUXING_APP: u32 = 0x4D80;
pub const WRITING_APP: u32 = 0x5741;

pub const CLUSTER: u32 = 0x1F43B675;
pub const TIMECODE: u32 = 0xE7;
pub const SIMPLE_BLOCK: u32 = 0xA3;
pub const BLOCK_GROUP: u32 = 0xA0;
pub const BLOCK: u32 = 0xA1;

pub const TRACKS: u32 = 0x1654AE6B;
pub const TRACK_ENTRY: u32 = 0xAE;
pub const TRACK_NUMBER: u32 = 0xD7;
pub const TRACK_UID: u32 = 0x73C5;
pub const TRACK_TYPE: u32 = 0x83;
pub const FLAG_ENABLED: u32 = 0xB9;
pub const FLAG_DEFAULT: u32 = 0x88;
pub const FLAG_FORCED: u32 = 0x55AA;
pub const FLAG_LACING: u32 = 0x9C;
pub const MIN_CACHE: u32 = 0x6DE7;
pub const MAX_CACHE: u32 = 0x6DF8;
pub const DEFAULT_DURATION: u32 = 0x23E383;
pub const DEFAULT_DECODED_FIELD_DURATION: u32 = 0x234E7A;
pub const TRACK_TIMECODE_SCALE: u32 = 0x23314F;
pub const MAX_BLOCK_ADDITION_ID: u32 = 0x55EE;
pub const NAME: u32 = 0x536E;
pub const LANGUAGE: u32 = 0x22B59C;
pub const CODEC_ID: u32 = 0x86;
pub const CODEC_PRIVATE: u32 = 0x63A2;
pub const CODEC_NAME: u32 = 0x258688;
pub const ATTACHMENT_LINK: u32 = 0x7446;
pub const TRACK_OVERLAY: u32 = 0x6FAB;
pub const VIDEO: u32 = 0xE0;
pub const FLAG_INTERLACED: u32 = 0x9A;
pub const STEREO_MODE: u32 = 0x53B8;
pub const ALPHA_MODE: u32 = 0x53C0;
pub const OLD_STEREO_MODE: u32 = 0x53B9;
pub const PIXEL_WIDTH: u32 = 0xB0;
pub const PIXEL_HEIGHT: u32 = 0xBA;
pub const PIXEL_CROP_BOTTOM: u32 = 0x54AA;
pub const PIXEL_CROP_TOP: u32 = 0x54BB;
pub const PIXEL_CROP_LEFT: u32 = 0x54CC;
pub const PIXEL_CROP_RIGHT: u32 = 0x54DD;
pub const DISPLAY_WIDTH: u32 = 0x54B0;
pub const DISPLAY_HEIGHT: u32 = 0x54BA;
pub const DISPLAY_UNIT: u32 = 0x54B2;
pub const ASPECT_RATIO_TYPE: u32 = 0x54B3;
pub const COLOUR_SPACE: u32 = 0x2EB524;
pub const GAMMA_VALUE: u32 = 0x2FB523;
pub const FRAME_RATE: u32 = 0x2383E3;
pub const AUDIO: u32 = 0xE1;
pub const SAMPLING_FREQUENCY: u32 = 0xB5;
pub const OUTPUT_SAMPLING_FREQUENCY: u32 = 0x78B5;
pub const CHANNELS: u32 = 0x9F;
pub const BIT_DEPTH: u32 = 0x6264;
pub const CONTENT_ENCODINGS: u32 = 0x6D80;
pub const CONTENT_ENCODING: u32 = 0x6240;
pub const CONTENT_ENCODING_ORDER: u32 = 0x5031;
pub const CONTENT_ENCODING_SCOPE: u32 = 0x5032;
pub const CONTENT_ENCODING_TYPE: u32 = 0x5033;
pub const CONTENT_COMPRESSION: u32 = 0x5034;
pub const CONTENT_COMP_ALGO: u32 = 0x4254;
pub const CONTENT_ENCRYPTION: u32 = 0x5035;
pub const CONTENT_ENC_ALGO: u32 = 0x47E1;
pub const CONTENT_ENC_KEY_ID: u32 = 0x47E2;
pub const CONTENT_SIGNATURE: u32 = 0x47E3;
pub const CONTENT_SIG_KEY_ID: u32 = 0x47E4;
pub const CONTENT_SIG_ALGO: u32 = 0x47E5;
pub const CONTENT_SIG_HASH_ALGO: u32 = 0x47E6;

pub const CUES: u32 = 0x1C53BB6B;
pub const CUE_POINT: u32 = 0xBB;
pub const CUE_TIME: u32 = 0xB3;
pub const CUE_TRACK_POSITIONS: u32 = 0xB7;
pub const CUE_TRACK: u32 = 0xF7;
pub const CUE_CLUSTER_POSITION: u32 = 0xF1;
pub const CUE_RELATIVE_POSITION: u32 = 0xF0;
pub const CUE_DURATION: u32 = 0xB2;
pub const CUE_BLOCK_NUMBER: u32 = 0x5378;
pub const CUE_CODEC_STATE: u32 = 0xEA;
pub const CUE_REFERENCE: u32 = 0xDB;
pub const CUE_REF_TIME: u32 = 0x96;
pub const CUE_REF_CLUSTER: u32 = 0x97;
pub const CUE_REF_NUMBER: u32 = 0x535F;
pub const CUE_REF_CODEC_STATE: u32 = 0xEB;

pub const ATTACHMENTS: u32 = 0x1941A469;
pub const ATTACHED_FILE: u32 = 0x61A7;
pub const FILE_DESCRIPTION: u32 = 0x467E;
pub const FILE_NAME: u32 = 0x466E;
pub const FILE_MIME_TYPE: u32 = 0x4660;
pub const FILE_DATA: u32 = 0x465C;
pub const FILE_UID: u32 = 0x46AE;

pub const CHAPTERS: u32 = 0x1043A770;
pub const EDITION_ENTRY: u32 = 0x45B9;
pub const EDITION_UID: u32 = 0x45BC;
pub const EDITION_FLAG_HIDDEN: u32 = 0x45BD;
pub const EDITION_FLAG_DEFAULT: u32 = 0x45DB;
pub const EDITION_FLAG_ORDERED: u32 = 0x45DD;
pub const CHAPTER_ATOM: u32 = 0xB6;
pub const CHAPTER_UID: u32 = 0x73C4;
pub const CHAPTER_STRING_UID: u32 = 0x5654;
pub const CHAPTER_TIME_START: u32 = 0x91;
pub const CHAPTER_TIME_END: u32 = 0x92;
pub const CHAPTER_FLAG_HIDDEN: u32 = 0x98;
pub const CHAPTER_FLAG_ENABLED: u32 = 0x4598;
pub const CHAPTER_SEGMENT_UID: u32 = 0x6E67;
pub const CHAPTER_SEGMENT_EDITION_UID: u32 = 0x6EBC;
pub const CHAPTER_PHYSICAL_EQUIV: u32 = 0x63C3;
pub const CHAPTER_TRACK: u32 = 0x8F;
pub const CHAPTER_TRACK_NUMBER: u32 = 0x89;
pub const CHAPTER_DISPLAY: u32 = 0x80;
pub const CHAP_STRING: u32 = 0x85;
pub const CHAP_LANGUAGE: u32 = 0x437C;
pub const CHAP_COUNTRY: u32 = 0x437E;
pub const CHAP_PROCESS: u32 = 0x6944;
pub const CHAP_PROCESS_CODEC_ID: u32 = 0x6955;
pub const CHAP_PROCESS_PRIVATE: u32 = 0x450D;
pub const CHAP_PROCESS_COMMAND: u32 = 0x6911;
pub const CHAP_PROCESS_TIME: u32 = 0x6922;
pub const CHAP_PROCESS_DATA: u32 = 0x6933;

pub const TAGS: u32 = 0x1254C367;
pub const TAG: u32 = 0x7373;
pub const TARGETS: u32 = 0x63C0;
pub const TARGET_TYPE_VALUE: u32 = 0x68CA;
pub const TARGET_TYPE: u32 = 0x63CA;
pub const TAG_TRACK_UID: u32 = 0x63C5;
pub const TAG_EDITION_UID: u32 = 0x63C9;
pub const TAG_CHAPTER_UID: u32 = 0x63C4;
pub const TAG_ATTACHMENT_UID: u32 = 0x63C6;
pub const SIMPLE_TAG: u32 = 0x67C8;
pub const TAG_NAME: u32 = 0x45A3;
pub const TAG_LANGUAGE: u32 = 0x447A;
pub const TAG_DEFAULT: u32 = 0x4484;
pub const TAG_STRING: u32 = 0x4487;
pub const TAG_BINARY: u32 = 0x4485;

/// Level-1 elements: encountering one of these ends an unknown-sized Cluster/Segment child scan.
pub const LEVEL1_ELEMENTS: &[u32] = &[SEEK_HEAD, INFO, CLUSTER, TRACKS, CUES, ATTACHMENTS, CHAPTERS, TAGS, EBML, SEGMENT];

/// Decide whether `child` terminates an unknown-sized master element `parent`.
pub fn unknown_size_terminator(parent: u32, child: u32) -> bool {
    match parent {
        SEGMENT => child == EBML || child == SEGMENT,
        CLUSTER => LEVEL1_ELEMENTS.contains(&child),
        _ => LEVEL1_ELEMENTS.contains(&child) || child == parent,
    }
}

pub fn is_global(id: u32) -> bool {
    id == VOID || id == CRC32
}
