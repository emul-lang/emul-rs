//! Fonts come from Google Fonts. The compiler reads the catalogue to check families and
//! faces. It downloads a font file into a cache when it needs the file.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

const CATALOGUE_URL: &str = "https://fonts.google.com/metadata/fonts";
const CSS_URL: &str = "https://fonts.googleapis.com/css2";
const CATALOGUE_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone, Debug)]
pub struct FamilyInfo {
    pub name: String,
    /// The static faces of the family: (weight, italic).
    pub faces: BTreeSet<(u16, bool)>,
    /// The weight axis range of a variable family.
    pub wght: Option<(u16, u16)>,
}

impl FamilyInfo {
    pub fn has_italic(&self) -> bool {
        self.faces.iter().any(|(_, i)| *i)
    }
    pub fn supports(&self, weight: u16, italic: bool) -> bool {
        if italic && !self.has_italic() {
            return false;
        }
        if self.faces.contains(&(weight, italic)) {
            return true;
        }
        matches!(self.wght, Some((lo, hi)) if weight >= lo && weight <= hi)
    }
    /// The closest face this family has, for fallback families.
    pub fn nearest(&self, weight: u16, italic: bool) -> (u16, bool) {
        let italic = italic && self.has_italic();
        if self.supports(weight, italic) {
            return (weight, italic);
        }
        if let Some((lo, hi)) = self.wght {
            return (weight.clamp(lo, hi), italic);
        }
        self.faces
            .iter()
            .filter(|(_, i)| *i == italic)
            .min_by_key(|(w, _)| (*w as i32 - weight as i32).abs())
            .copied()
            .unwrap_or((400, false))
    }
}

pub struct Catalogue {
    families: HashMap<String, FamilyInfo>,
    lower: HashMap<String, String>,
}

#[derive(Debug)]
pub enum FontError {
    UnknownFamily(String, Option<String>),
    MissingFace { family: String, weight: u16, italic: bool },
    Unavailable(String),
}

impl Catalogue {
    pub fn family(&self, name: &str) -> Result<&FamilyInfo, FontError> {
        if let Some(f) = self.families.get(name) {
            return Ok(f);
        }
        let hint = self.lower.get(&name.to_lowercase()).cloned();
        Err(FontError::UnknownFamily(name.to_string(), hint))
    }

    pub fn load(cache: &Path) -> Result<Catalogue, FontError> {
        let path = cache.join("catalogue.json");
        let fresh = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age < CATALOGUE_MAX_AGE);
        let text = if fresh {
            std::fs::read_to_string(&path).ok()
        } else {
            match fetch_text(CATALOGUE_URL) {
                Ok(t) => {
                    let _ = std::fs::create_dir_all(cache);
                    let _ = std::fs::write(&path, &t);
                    Some(t)
                }
                // Offline: use an old copy if there is one.
                Err(_) => std::fs::read_to_string(&path).ok(),
            }
        };
        let Some(text) = text else {
            return Err(FontError::Unavailable("the Google Fonts catalogue can't be downloaded, and there is no cached copy".into()));
        };
        Self::parse(&text).ok_or_else(|| FontError::Unavailable("the Google Fonts catalogue could not be read".into()))
    }

    fn parse(text: &str) -> Option<Catalogue> {
        let start = text.find('{')?;
        let v: serde_json::Value = serde_json::from_str(&text[start..]).ok()?;
        let mut families = HashMap::new();
        let mut lower = HashMap::new();
        for f in v.get("familyMetadataList")?.as_array()? {
            let name = f.get("family")?.as_str()?.to_string();
            let mut faces = BTreeSet::new();
            if let Some(map) = f.get("fonts").and_then(|x| x.as_object()) {
                for key in map.keys() {
                    let italic = key.ends_with('i');
                    if let Ok(w) = key.trim_end_matches('i').parse::<u16>() {
                        faces.insert((w, italic));
                    }
                }
            }
            let mut wght = None;
            if let Some(axes) = f.get("axes").and_then(|x| x.as_array()) {
                for a in axes {
                    if a.get("tag").and_then(|t| t.as_str()) == Some("wght") {
                        let lo = a.get("min").and_then(|x| x.as_f64()).unwrap_or(400.0) as u16;
                        let hi = a.get("max").and_then(|x| x.as_f64()).unwrap_or(400.0) as u16;
                        wght = Some((lo, hi));
                    }
                }
            }
            lower.insert(name.to_lowercase(), name.clone());
            families.insert(name.clone(), FamilyInfo { name, faces, wght });
        }
        Some(Catalogue { families, lower })
    }
}

fn agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(60))).build();
    ureq::Agent::new_with_config(config)
}

fn fetch_text(url: &str) -> Result<String, String> {
    let mut resp = agent().get(url).call().map_err(|e| e.to_string())?;
    resp.body_mut().with_config().limit(64 * 1024 * 1024).read_to_string().map_err(|e| e.to_string())
}

fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    let mut resp = agent().get(url).call().map_err(|e| e.to_string())?;
    resp.body_mut().with_config().limit(256 * 1024 * 1024).read_to_vec().map_err(|e| e.to_string())
}

/// One face to load: a family at a weight and style.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FaceKey {
    pub family: String,
    pub weight: u16,
    pub italic: bool,
}

#[derive(Clone)]
pub struct LoadedFace {
    pub key: FaceKey,
    pub data: Arc<Vec<u8>>,
}

fn family_param(name: &str) -> String {
    name.replace(' ', "+")
}

/// Downloads (or reads from the cache) the file for one face.
pub fn face_file(cache: &Path, key: &FaceKey) -> Result<Arc<Vec<u8>>, FontError> {
    let dir = cache.join("fonts").join(key.family.replace(['/', '\\'], "_"));
    let file = dir.join(format!("{}{}.ttf", key.weight, if key.italic { "i" } else { "" }));
    if let Ok(data) = std::fs::read(&file) {
        return Ok(Arc::new(data));
    }
    let url = format!("{CSS_URL}?family={}:ital,wght@{},{}", family_param(&key.family), key.italic as u8, key.weight);
    let unavailable = |e: String| FontError::Unavailable(format!("{} {}{}: {e}", key.family, key.weight, if key.italic { " italic" } else { "" }));
    // Without a browser user agent, Google Fonts serves TrueType files.
    let css = fetch_text(&url).map_err(unavailable)?;
    let font_url = css
        .split("url(")
        .nth(1)
        .and_then(|s| s.split(')').next())
        .ok_or_else(|| unavailable("Google Fonts returned no font file".into()))?;
    let data = fetch_bytes(font_url).map_err(unavailable)?;
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(&file, &data);
    Ok(Arc::new(data))
}

/// The fonts that a set of compositions needs. The code collects them from the text.
#[derive(Default, Debug, Clone)]
pub struct FontNeeds {
    /// For each (family list, weight, italic) that some text uses.
    pub uses: BTreeSet<(Vec<String>, u16, bool)>,
}

pub struct FontSet {
    pub faces: Vec<LoadedFace>,
    /// The families and the faces that the text uses, for the HTML stylesheet link.
    pub families: BTreeMap<String, BTreeSet<(u16, bool)>>,
}

impl FontSet {
    pub fn empty() -> FontSet {
        FontSet { faces: Vec::new(), families: BTreeMap::new() }
    }

    /// The Google Fonts stylesheet URL for the faces that the text uses, and no others.
    pub fn stylesheet_url(&self) -> Option<String> {
        if self.families.is_empty() {
            return None;
        }
        let mut url = String::from(CSS_URL);
        let mut first = true;
        for (fam, faces) in &self.families {
            url.push(if first { '?' } else { '&' });
            first = false;
            let specs: Vec<String> = faces.iter().map(|(w, i)| format!("{},{}", *i as u8, w)).collect();
            url.push_str(&format!("family={}:ital,wght@{}", family_param(fam), specs.join(";")));
        }
        url.push_str("&display=block");
        Some(url)
    }
}

/// Checks every use against the catalogue and loads the faces that the text needs. It returns
/// each error as a (code, message) pair, with the family of the error.
pub fn resolve(needs: &FontNeeds, cache: &Path) -> Result<FontSet, Vec<(&'static str, String, String)>> {
    let cat = match Catalogue::load(cache) {
        Ok(c) => c,
        Err(FontError::Unavailable(m)) => return Err(vec![("font-unavailable", m, String::new())]),
        Err(_) => unreachable!(),
    };
    let mut errors = Vec::new();
    let mut wanted: BTreeSet<FaceKey> = BTreeSet::new();
    for (fams, weight, italic) in &needs.uses {
        for (i, fam) in fams.iter().enumerate() {
            match cat.family(fam) {
                Ok(info) => {
                    if i == 0 {
                        if info.supports(*weight, *italic) {
                            wanted.insert(FaceKey { family: info.name.clone(), weight: *weight, italic: *italic });
                        } else {
                            errors.push((
                                "missing-font-face",
                                format!(
                                    "Google Fonts' `{fam}` has no {}{} face",
                                    weight,
                                    if *italic { " italic" } else { "" }
                                ),
                                fam.clone(),
                            ));
                        }
                    } else {
                        let (w, it) = info.nearest(*weight, *italic);
                        wanted.insert(FaceKey { family: info.name.clone(), weight: w, italic: it });
                    }
                }
                Err(FontError::UnknownFamily(name, hint)) => {
                    let hint = hint.map(|h| format!("; did you mean `{h}`?")).unwrap_or_default();
                    errors.push(("unknown-font-family", format!("Google Fonts has no family named `{name}`{hint}"), fam.clone()));
                }
                Err(_) => {}
            }
        }
    }
    if !errors.is_empty() {
        errors.sort();
        errors.dedup();
        return Err(errors);
    }
    let mut set = FontSet::empty();
    // Download in parallel, because there can be several faces.
    let keys: Vec<FaceKey> = wanted.into_iter().collect();
    let results: Vec<(FaceKey, Result<Arc<Vec<u8>>, FontError>)> = std::thread::scope(|s| {
        let handles: Vec<_> = keys.iter().map(|k| (k.clone(), s.spawn(|| face_file(cache, k)))).collect();
        handles.into_iter().map(|(k, h)| (k, h.join().unwrap_or_else(|_| Err(FontError::Unavailable("download failed".into()))))).collect()
    });
    for (key, r) in results {
        match r {
            Ok(data) => {
                set.families.entry(key.family.clone()).or_default().insert((key.weight, key.italic));
                set.faces.push(LoadedFace { key, data });
            }
            Err(FontError::Unavailable(m)) => errors.push(("font-unavailable", m, key.family.clone())),
            Err(_) => {}
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(set)
}

pub fn cache_dir(configured: &Path) -> PathBuf {
    configured.to_path_buf()
}
