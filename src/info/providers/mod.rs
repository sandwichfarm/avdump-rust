//! Info providers: each turns one source (a consumer result or the file itself) into a
//! [`MetaProvider`] tree.

pub mod format;
pub mod hash;
pub mod matroska;
pub mod mediainfo;
pub mod mp4;
pub mod ogg;

use super::meta::MetaProvider;
use crate::processing::consumers::BlockConsumer;
use std::path::Path;

/// Everything a provider factory may look at.
pub struct InfoProviderSetup<'a> {
    pub file_path: &'a Path,
    pub block_consumers: &'a [Box<dyn BlockConsumer>],
}

impl<'a> InfoProviderSetup<'a> {
    /// First consumer of the given concrete type.
    pub fn consumer<T: 'static>(&self) -> Option<&'a T> {
        self.block_consumers.iter().find_map(|c| c.as_any().downcast_ref::<T>())
    }
    pub fn consumers<T: 'static>(&self) -> Vec<&'a T> {
        self.block_consumers.iter().filter_map(|c| c.as_any().downcast_ref::<T>()).collect()
    }
}

pub type CreateInfoProvider = dyn Fn(&InfoProviderSetup<'_>) -> Option<MetaProvider> + Send + Sync;

pub struct InfoProviderFactory {
    pub name: &'static str,
    create: Box<CreateInfoProvider>,
}

impl InfoProviderFactory {
    pub fn new(name: &'static str, create: impl Fn(&InfoProviderSetup<'_>) -> Option<MetaProvider> + Send + Sync + 'static) -> Self {
        Self { name, create: Box::new(create) }
    }
    pub fn create(&self, setup: &InfoProviderSetup<'_>) -> Option<MetaProvider> {
        (self.create)(setup)
    }
}

/// The default provider set, in the order of the original information module.
pub fn default_info_provider_factories() -> Vec<InfoProviderFactory> {
    vec![
        InfoProviderFactory::new("MP4Provider", |s| Some(mp4::Mp4Provider::create(s.consumer::<crate::processing::consumers::mp4::Mp4Parser>().and_then(|p| p.root_box())))),
        InfoProviderFactory::new("MatroskaProvider", |s| Some(matroska::MatroskaProvider::create(s.consumer::<crate::processing::consumers::matroska::MatroskaParser>().and_then(|p| p.info())))),
        InfoProviderFactory::new("OggProvider", |s| Some(ogg::OggProvider::create(s.consumer::<crate::processing::consumers::ogg::OggParser>().and_then(|p| p.info())))),
        InfoProviderFactory::new("FormatInfoProvider", |s| Some(format::FormatInfoProvider::create(s.file_path))),
        InfoProviderFactory::new("MediaInfoLibProvider", |s| mediainfo::MediaInfoLibProvider::create(s.file_path)),
        InfoProviderFactory::new("HashProvider", |s| Some(hash::HashProvider::create(&s.consumers::<crate::processing::consumers::HashCalculator>()))),
    ]
}
