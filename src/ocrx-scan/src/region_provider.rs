//! Region provider port (m4-table-reopen plan R2.1, route A per the R1
//! adjudication): page regions come from the batch pre-detection sidecar,
//! which writes one regions JSON per document. The provider READS that
//! file — detection never runs on the scan hot path.
//!
//! Degradation semantics (frozen): missing file, malformed JSON, or a page
//! without an entry all yield EMPTY regions — the reread pipeline then
//! skips the page (original output preserved); the scan itself never
//! fails on region-provider problems.
//!
//! File schema (frozen, written by the sidecar with the R1b normalization
//! already applied — the provider trusts ordinals and only maps classes):
//!
//! ```json
//! {"schema_version": 1,
//!  "model_id": "pp-doclayout-v2",
//!  "revision": "<frozen>",
//!  "pages": [{"page": 1,
//!             "regions": [{"class": "table", "index_in_class": 0,
//!                          "bbox_px": [x0, y0, x1, y1], "conf": 0.93}]}]}
//! ```

use serde::Deserialize;

use crate::reread::{DetectedRegion, RegionClass};

#[derive(Debug, Deserialize)]
struct RegionsFile {
    #[serde(default)]
    pages: Vec<PageEntry>,
}

#[derive(Debug, Deserialize)]
struct PageEntry {
    page: u32,
    #[serde(default)]
    regions: Vec<RegionEntry>,
}

#[derive(Debug, Deserialize)]
struct RegionEntry {
    class: String,
    index_in_class: usize,
    bbox_px: [f64; 4],
    #[serde(default)]
    conf: f64,
}

/// Port consumed by the reread orchestration (R2.4). `regions_for_page`
/// returns regions in provider (file) order — the pairing verdict owns
/// ordering decisions; the provider never re-sorts.
pub trait RegionProvider {
    fn regions_for_page(&mut self, page: u32) -> Vec<DetectedRegion>;
}

/// File-backed provider. Constructed from the per-document regions file;
/// any read/parse problem degrades to an empty provider (scan never fails).
pub struct FileRegionProvider {
    pages: std::collections::BTreeMap<u32, Vec<DetectedRegion>>,
}

impl FileRegionProvider {
    /// Reads the regions file. Missing or malformed file => empty provider
    /// (degradation, never an error — plan R2.1 frozen semantics).
    #[must_use]
    pub fn open(path: impl AsRef<std::path::Path>) -> Self {
        match Self::parse(path.as_ref()) {
            Ok(pages) => Self { pages },
            Err(_) => Self {
                pages: std::collections::BTreeMap::new(),
            },
        }
    }

    /// R3 orchestrated form: `root` is a REGIONS DIRECTORY — the document's
    /// file is `<root>/<input stem>.regions.json` (the sidecar writes one
    /// file per PDF). A FILE path keeps the single-document form. Both
    /// degrade to empty on any read problem.
    #[must_use]
    pub fn for_document(root: impl AsRef<std::path::Path>, input_stem: &str) -> Self {
        let root = root.as_ref();
        let path = if root.is_dir() {
            root.join(format!("{input_stem}.regions.json"))
        } else {
            root.to_path_buf()
        };
        Self::open(path)
    }

    fn parse(
        path: &std::path::Path,
    ) -> std::io::Result<std::collections::BTreeMap<u32, Vec<DetectedRegion>>> {
        let text = std::fs::read_to_string(path)?;
        let file: RegionsFile = serde_json::from_str(&text).map_err(|error| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
        })?;
        let mut pages = std::collections::BTreeMap::new();
        for entry in file.pages {
            let regions: Vec<DetectedRegion> = entry
                .regions
                .into_iter()
                .filter_map(|r| match r.class.as_str() {
                    "table" => Some(DetectedRegion {
                        class: RegionClass::Table,
                        index_in_class: r.index_in_class,
                        bbox: r.bbox_px.into(),
                        conf: r.conf,
                    }),
                    _ => None, // other classes are outside this plan's scope
                })
                .collect();
            pages.insert(entry.page, regions);
        }
        Ok(pages)
    }
}

impl RegionProvider for FileRegionProvider {
    fn regions_for_page(&mut self, page: u32) -> Vec<DetectedRegion> {
        self.pages.get(&page).cloned().unwrap_or_default()
    }
}

/// Always-empty provider: the degradation shape (and the off-flag shape —
/// R2.6 wires the off switch to this).
#[derive(Debug, Default)]
pub struct EmptyRegionProvider;

impl RegionProvider for EmptyRegionProvider {
    fn regions_for_page(&mut self, _page: u32) -> Vec<DetectedRegion> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_file(dir: &std::path::Path, name: &str, content: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn file_provider_maps_table_regions_to_detected_regions() {
        let dir = std::env::temp_dir().join(format!("ocrx-regprov-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = write_file(
            &dir,
            "regions.json",
            r#"{"schema_version":1,"model_id":"pp-doclayout-v2","revision":"r1","pages":[
                {"page":1,"regions":[
                    {"class":"table","index_in_class":0,"bbox_px":[10,20,300,400],"conf":0.95},
                    {"class":"formula","index_in_class":0,"bbox_px":[5,5,50,60],"conf":0.9},
                    {"class":"table","index_in_class":1,"bbox_px":[10,500,300,800],"conf":0.88}]},
                {"page":2,"regions":[]}]}"#,
        );
        let mut provider = FileRegionProvider::open(&path);
        let regions = provider.regions_for_page(1);
        assert_eq!(regions.len(), 2, "non-table classes dropped");
        // file order preserved (provider never re-sorts; pairing owns order)
        assert_eq!(regions[0].index_in_class, 0);
        assert_eq!(regions[0].class, RegionClass::Table);
        assert_eq!(regions[0].bbox, (10.0, 20.0, 300.0, 400.0));
        assert_eq!(regions[1].index_in_class, 1);
        // page 2 has an entry with no regions -> empty
        assert!(provider.regions_for_page(2).is_empty());
        // unknown page -> empty (degradation)
        assert!(provider.regions_for_page(99).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn for_document_resolves_the_stem_under_a_regions_root() {
        let dir = std::env::temp_dir().join(format!("ocrx-regprov-root-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        write_file(
            &dir,
            "my-doc.regions.json",
            r#"{"schema_version":1,"model_id":"pp-doclayout-v2","revision":"r1","pages":[
                {"page":1,"regions":[{"class":"table","index_in_class":0,"bbox_px":[1,2,3,4],"conf":0.9}]}]}"#,
        );
        let mut provider = FileRegionProvider::for_document(&dir, "my-doc");
        assert_eq!(provider.regions_for_page(1).len(), 1);
        // Unknown stem / missing file degrade to empty.
        let mut missing = FileRegionProvider::for_document(&dir, "other-doc");
        assert!(missing.regions_for_page(1).is_empty());
        // FILE form still works (single-document invocation).
        let file = dir.join("my-doc.regions.json");
        let mut direct = FileRegionProvider::for_document(&file, "ignored");
        assert_eq!(direct.regions_for_page(1).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_or_malformed_file_degrades_to_empty() {
        let mut missing = FileRegionProvider::open(
            std::env::temp_dir().join(format!("ocrx-regprov-missing-{}", std::process::id())),
        );
        assert!(missing.regions_for_page(1).is_empty());
        let dir = std::env::temp_dir().join(format!("ocrx-regprov-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = write_file(&dir, "broken.json", "{not json at all");
        let mut malformed = FileRegionProvider::open(&path);
        assert!(malformed.regions_for_page(1).is_empty());
        assert!(EmptyRegionProvider.regions_for_page(1).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
