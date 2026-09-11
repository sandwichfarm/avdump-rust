//! Report generation (AVD3 XML, Matroska structure dump, raw MediaInfo XML).

use crate::info::meta::MetaInfoContainer;
use crate::info::value::Value;
use crate::info::FileMetaInfo;
use crate::misc::xml::XElement;
use crate::processing::consumers::matroska::MatroskaParser;
use std::sync::Arc;

/// A generated report for one file.
pub struct Report {
    pub file_extension: &'static str,
    root: XElement,
}

impl Report {
    pub fn to_report_string(&self) -> String {
        self.root.to_string_indented()
    }

    /// Append the report to `file_path` (creating directories as needed).
    pub fn save_to_file(&self, file_path: &std::path::Path, content_prefix: &str) -> std::io::Result<()> {
        use std::io::Write;
        if let Some(parent) = file_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(file_path)?;
        if !content_prefix.is_empty() {
            f.write_all(content_prefix.as_bytes())?;
            f.write_all(b"\n")?;
        }
        f.write_all(self.to_report_string().as_bytes())?;
        f.write_all(b"\n\n")?;
        f.flush()
    }
}

pub type CreateReport = dyn Fn(&FileMetaInfo, &[Box<dyn crate::processing::consumers::BlockConsumer>]) -> Report + Send + Sync;

#[derive(Clone)]
pub struct ReportFactory {
    pub name: String,
    pub description: String,
    create: Arc<CreateReport>,
}

impl ReportFactory {
    pub fn new(name: &str, description: &str, create: impl Fn(&FileMetaInfo, &[Box<dyn crate::processing::consumers::BlockConsumer>]) -> Report + Send + Sync + 'static) -> Self {
        Self { name: name.to_string(), description: description.to_string(), create: Arc::new(create) }
    }
    pub fn create(&self, fmi: &FileMetaInfo, consumers: &[Box<dyn crate::processing::consumers::BlockConsumer>]) -> Report {
        (self.create)(fmi, consumers)
    }
}

pub fn default_report_factories() -> Vec<ReportFactory> {
    vec![
        ReportFactory::new("AVD3", "Complete xml report with every available information node", |fmi, _| avd3_report(fmi)),
        ReportFactory::new("MediaInfoXml", "Raw MediaInfoLibrary Output", |fmi, _| Report { file_extension: "xml", root: crate::info::providers::mediainfo::xml_report(&fmi.path) }),
        ReportFactory::new("Matroska", "Matroska Structure Report", |_, consumers| matroska_report(consumers)),
    ]
}

/// The complete metadata dump (`AVD3Report`).
pub fn avd3_report(fmi: &FileMetaInfo) -> Report {
    let mut root = XElement::new("FileInfo");
    root.add(XElement::with_text("Path", fmi.full_name()));
    root.add(XElement::with_text("Size", fmi.resolved_length.to_string()));
    for provider in &fmi.condensed_providers {
        root.add(build_report_media(&provider.root));
    }
    Report { file_extension: "xml", root }
}

fn build_report_media(container: &MetaInfoContainer) -> XElement {
    let mut root = XElement::new(&container.container_type);
    for item in &container.items {
        let mut e = XElement::new(&item.key).attr("p", &item.provider).attr("t", item.value.type_name()).attr("u", &item.unit);
        match &item.value {
            Value::Binary(b) => e.set_text(crate::hashes::to_hex_upper(b)),
            v if v.is_collection() => {
                for x in v.collection_items() {
                    e.add(XElement::with_text("Item", x));
                }
            }
            v => e.set_text(v.to_display_string()),
        }
        root.add(e);
    }
    for node in &container.nodes {
        root.add(build_report_media(node));
    }
    root
}

/// Structural dump of the Matroska parser result (`MatroskaReport`).
pub fn matroska_report(consumers: &[Box<dyn crate::processing::consumers::BlockConsumer>]) -> Report {
    let parser = consumers.iter().find_map(|c| c.as_any().downcast_ref::<MatroskaParser>());
    let root = match parser.and_then(|p| p.info()) {
        Some(info) if info.has_meta_data() => info.to_xml(),
        _ => XElement::new("File"),
    };
    Report { file_extension: "xml", root }
}
