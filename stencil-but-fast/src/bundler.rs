use anyhow::{Context, Result};
use rayon::prelude::*;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

pub struct BundleOptions {
    pub theme_path: PathBuf,
    pub output_path: PathBuf,
    pub source_maps: bool,
}

pub struct BundleResult {
    pub path: PathBuf,
    pub file_count: usize,
    pub size_bytes: u64,
}

/// Create a theme bundle ZIP.
///
/// This is CPU/IO-bound — call it from `tokio::task::spawn_blocking`.
/// Uses rayon to read all files in parallel, then writes them sequentially
/// into a Deflate-compressed ZIP for maximum throughput.
pub fn create_bundle(opts: &BundleOptions) -> Result<BundleResult> {
    let theme_path = &opts.theme_path;

    // Step 1: collect (relative_path, absolute_path) pairs
    let paths = collect_paths(theme_path, opts.source_maps)
        .context("Failed to collect theme files")?;

    // Step 2: read all file contents in parallel with rayon
    let file_data: Vec<(String, Vec<u8>)> = paths
        .par_iter()
        .map(|(rel, abs)| {
            let data = std::fs::read(abs).unwrap_or_default();
            (rel.clone(), data)
        })
        .collect();

    // Step 3: generate manifest from template data already in memory (zero extra I/O)
    let manifest = generate_manifest(&file_data);

    // Step 4: write ZIP sequentially
    // On Windows, use the \\?\ extended-length prefix so paths > 260 chars work.
    let create_path = long_path(&opts.output_path);
    let out = File::create(&create_path).with_context(|| {
        format!("Cannot create bundle at {}", opts.output_path.display())
    })?;
    let mut zip = ZipWriter::new(out);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(6));

    let mut file_count = 0;
    for (rel, data) in &file_data {
        zip.start_file(rel, options)
            .with_context(|| format!("Failed to add {} to ZIP", rel))?;
        zip.write_all(data)?;
        file_count += 1;
    }

    // Inject generated manifest.json
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    zip.start_file("manifest.json", options)?;
    zip.write_all(&manifest_bytes)?;

    zip.finish().context("Failed to finalise ZIP")?;

    let size_bytes = std::fs::metadata(&opts.output_path)
        .map(|m| m.len())
        .unwrap_or(0);

    Ok(BundleResult {
        path: opts.output_path.clone(),
        file_count,
        size_bytes,
    })
}

// ── Filename sanitization ─────────────────────────────────────────────────────

/// Replace characters that are invalid in Windows filenames with `-`.
/// The set `< > : " / \ | ? *` plus ASCII control chars is forbidden on Windows;
/// we sanitize on all platforms for portability.
pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_ascii_control() => '-',
            c => c,
        })
        .collect()
}

// ── Windows long-path helper ──────────────────────────────────────────────────

/// Prefix an absolute path with `\\?\` on Windows so paths longer than MAX_PATH
/// (260 chars) work correctly. No-op on other platforms.
fn long_path(p: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let s = p.to_string_lossy();
        if !s.starts_with(r"\\") {
            return PathBuf::from(format!(r"\\?\{}", s));
        }
    }
    p.to_path_buf()
}

// ── File collection ───────────────────────────────────────────────────────────

fn collect_paths(theme_path: &Path, source_maps: bool) -> Result<Vec<(String, PathBuf)>> {
    let mut result = Vec::new();
    for entry in WalkDir::new(theme_path)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let abs = entry.path().to_path_buf();
        let rel = abs.strip_prefix(theme_path).context("strip_prefix failed")?;
        let rel_str = rel.to_string_lossy().replace('\\', "/");

        if should_include(&rel_str, source_maps) {
            result.push((rel_str, abs));
        }
    }
    Ok(result)
}

fn should_include(rel: &str, source_maps: bool) -> bool {
    // Skip version-control and dependency directories
    if rel.starts_with(".git/")
        || rel.starts_with("node_modules/")
        || rel.starts_with(".stencil/")
    {
        return false;
    }
    // Skip CDN-cached assets (large, not needed in bundle)
    if rel.starts_with("assets/cdn/") {
        return false;
    }
    // Source maps are opt-in
    if !source_maps && rel.ends_with(".js.map") {
        return false;
    }

    // Directory prefixes always included
    if rel.starts_with("assets/")
        || rel.starts_with("templates/")
        || rel.starts_with("lang/")
        || rel.starts_with("meta/")
    {
        return true;
    }

    // Well-known root-level files
    matches!(
        rel,
        "config.json"
            | "schema.json"
            | "schemaTranslations.json"
            | "package.json"
            | "README.md"
            | "CHANGELOG.md"
            | "stencil.conf.js"
            | "stencil.conf.cjs"
            | ".eslintrc"
            | ".eslintignore"
            | ".scss-lint.yml"
            | "Gruntfile.js"
            | "karma.conf.js"
    ) || (rel.starts_with("webpack.") && rel.ends_with(".js"))
}

// ── Manifest generation ───────────────────────────────────────────────────────

/// Generates `manifest.json` by scanning template files for `{{{region name="..."}}}`.
fn generate_manifest(file_data: &[(String, Vec<u8>)]) -> Value {
    // Matches both single and double quoted region names
    let region_re =
        Regex::new(r#"\{\{\{region\s+name=["']([^"']+)["']\s*\}\}\}"#).unwrap();

    let mut templates: Vec<String> = Vec::new();
    let mut regions: HashMap<String, Vec<String>> = HashMap::new();

    for (rel, data) in file_data {
        if rel.starts_with("templates/") && rel.ends_with(".html") {
            templates.push(rel.clone());
            if let Ok(content) = std::str::from_utf8(data) {
                let found: HashSet<String> = region_re
                    .captures_iter(content)
                    .map(|cap| cap[1].to_string())
                    .collect();
                if !found.is_empty() {
                    let mut v: Vec<String> = found.into_iter().collect();
                    v.sort();
                    regions.insert(rel.clone(), v);
                }
            }
        }
    }

    templates.sort();

    json!({
        "templates": templates,
        "regions": regions,
    })
}
