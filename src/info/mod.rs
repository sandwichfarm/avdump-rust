//! Information module: turns consumer results (and the file itself) into a metadata tree.

pub mod file_meta_info;
pub mod meta;
pub mod providers;
pub mod value;

pub use file_meta_info::FileMetaInfo;
pub use meta::{MetaInfoContainer, MetaInfoItem, MetaProvider};
pub use value::Value;
