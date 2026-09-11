//! Generic metadata containers (`MetaInfoContainer` / `MetaInfoItem` / `MetaDataProvider`).

use super::value::Value;

/// Container type names.
pub mod container_types {
    pub const MEDIA_PROVIDER: &str = "MediaProvider";
    pub const HASH_PROVIDER: &str = "HashProvider";
    pub const MEDIA_STREAM: &str = "MediaStream";
    pub const AUDIO_STREAM: &str = "AudioStream";
    pub const VIDEO_STREAM: &str = "VideoStream";
    pub const SUBTITLE_STREAM: &str = "SubtitleStream";
    pub const ATTACHMENT: &str = "Attachment";
    pub const CHAPTERS: &str = "Chapters";
    pub const CHAPTER: &str = "Chapter";
}

/// Item type keys (with their units) used across providers.
pub mod keys {
    pub const DIMENSIONLESS: &str = "Dimensionsless";
    pub const BYTES: &str = "bytes";
    pub const SECONDS: &str = "s";
    pub const HZ: &str = "s^-1";
    pub const BITS_PER_SECOND: &str = "bit/s";
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetaInfoItem {
    pub key: String,
    pub unit: String,
    pub value: Value,
    pub provider: String,
    pub notes: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default)]
pub struct MetaInfoContainer {
    pub id: u64,
    pub container_type: String,
    pub items: Vec<MetaInfoItem>,
    pub nodes: Vec<MetaInfoContainer>,
}

impl MetaInfoContainer {
    pub fn new(id: u64, container_type: &str) -> Self {
        Self { id, container_type: container_type.to_string(), items: Vec::new(), nodes: Vec::new() }
    }

    /// Add an item; the first item per key wins (mirrors `KeyedCollection` semantics).
    pub fn add_item(&mut self, item: MetaInfoItem) -> bool {
        if self.items.iter().any(|i| i.key == item.key && i.value.type_name() == item.value.type_name()) {
            return false;
        }
        self.items.push(item);
        true
    }

    pub fn add_node(&mut self, node: MetaInfoContainer) {
        self.nodes.push(node);
    }

    pub fn select(&self, key: &str) -> Option<&MetaInfoItem> {
        self.items.iter().find(|i| i.key == key)
    }

    pub fn count_nodes(&self, container_type: &str) -> usize {
        self.nodes.iter().filter(|n| n.container_type == container_type).count()
    }
}

/// A named provider owning a metadata tree.
#[derive(Debug, Clone)]
pub struct MetaProvider {
    pub name: String,
    pub root: MetaInfoContainer,
}

impl MetaProvider {
    pub fn new(name: &str, container_type: &str) -> Self {
        Self { name: name.to_string(), root: MetaInfoContainer::new(0, container_type) }
    }

    pub fn container_type(&self) -> &str {
        &self.root.container_type
    }

    /// Add an item to the root container.
    pub fn add(&mut self, key: &str, unit: &str, value: Value) -> bool {
        let item = MetaInfoItem { key: key.to_string(), unit: unit.to_string(), value, provider: self.name.clone(), notes: Vec::new() };
        self.root.add_item(item)
    }

    /// Add an optional item to the root container.
    pub fn add_opt(&mut self, key: &str, unit: &str, value: Option<Value>) -> bool {
        match value {
            Some(v) => self.add(key, unit, v),
            None => false,
        }
    }

    /// Add an item to an arbitrary container, attributed to this provider.
    pub fn add_to(&self, container: &mut MetaInfoContainer, key: &str, unit: &str, value: Option<Value>) -> bool {
        self.add_to_with_notes(container, key, unit, value, Vec::new())
    }

    pub fn add_to_with_notes(&self, container: &mut MetaInfoContainer, key: &str, unit: &str, value: Option<Value>, notes: Vec<(String, String)>) -> bool {
        match value {
            Some(v) => container.add_item(MetaInfoItem { key: key.to_string(), unit: unit.to_string(), value: v, provider: self.name.clone(), notes }),
            None => false,
        }
    }

    pub fn add_node(&mut self, node: MetaInfoContainer) {
        self.root.add_node(node);
    }

    pub fn select(&self, key: &str) -> Option<&MetaInfoItem> {
        self.root.select(key)
    }

    /// Merge several providers of the same container type into one tree (`CompositeMetaDataProvider`).
    pub fn composite(name: &str, providers: &[&MetaProvider]) -> MetaProvider {
        let container_type = providers.first().map(|p| p.container_type()).unwrap_or(container_types::MEDIA_PROVIDER);
        let mut out = MetaProvider::new(name, container_type);
        let roots: Vec<&MetaInfoContainer> = providers.iter().map(|p| &p.root).collect();
        choose_from(&roots, &mut out.root);
        out
    }
}

fn choose_from(sources: &[&MetaInfoContainer], dest: &mut MetaInfoContainer) {
    for container in sources {
        for item in &container.items {
            dest.add_item(item.clone());
        }
    }
    // Group child nodes by (id, type) preserving first-seen order.
    let mut groups: Vec<((u64, String), Vec<&MetaInfoContainer>)> = Vec::new();
    for container in sources {
        for node in &container.nodes {
            let key = (node.id, node.container_type.clone());
            match groups.iter_mut().find(|(k, _)| *k == key) {
                Some((_, v)) => v.push(node),
                None => groups.push((key, vec![node])),
            }
        }
    }
    for ((id, ty), nodes) in groups {
        let mut sub = MetaInfoContainer::new(id, &ty);
        choose_from(&nodes, &mut sub);
        dest.add_node(sub);
    }
}
