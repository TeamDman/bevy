use cloud_terrastodon_user_input::{Choice, PickerTui};
use color_eyre::eyre::{eyre, WrapErr};
use color_eyre::Result;
use serde::{Deserialize, Serialize};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;
use walkdir::WalkDir;

#[derive(Serialize, Deserialize, Default)]
struct History {
    statuses: HashMap<String, bool>,
}

impl History {
    fn insert(&mut self, canonical: &Path, ice_found: bool) {
        self.statuses
            .insert(canonical.to_string_lossy().to_string(), ice_found);
    }

    fn get(&self, canonical: &Path) -> Option<bool> {
        self.statuses
            .get(&canonical.to_string_lossy().to_string())
            .copied()
    }
}

struct FileEntry {
    relative: PathBuf,
    canonical: PathBuf,
    selectable: bool,
}

struct FormatResult {
    ice_found: bool,
    success: bool,
    output: String,
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let bevy_reflect_dir = manifest_dir
        .parent()
        .ok_or_else(|| eyre!("unable to locate parent directory"))?;
    let workspace_root = bevy_reflect_dir
        .parent()
        .and_then(|p| p.parent())
        .ok_or_else(|| eyre!("unable to locate workspace root"))?
        .canonicalize()
        .wrap_err("failed to canonicalize workspace root")?;
    let derive_dir = bevy_reflect_dir.join("derive");
    let derive_dir = derive_dir
        .canonicalize()
        .wrap_err("failed to canonicalize derive directory")?;
    let src_root = derive_dir.join("src");
    let history_path = manifest_dir.join("history.json");

    let mut history = load_history(&history_path)?;
    let entries = collect_files(&src_root)?;
    let (choices, has_choices) = build_choices(&entries, &history);

    if !has_choices {
        println!("No selectable files found under {}", src_root.display());
        return Ok(());
    }

    let chosen_indices = PickerTui::<usize>::new(choices)
        .set_header("Select the files to include in the next cargo fmt run")
        .pick_many()
        .map_err(|picker_err| eyre!("picker cancelled: {picker_err}"))?;

    let selected_entries: Vec<&FileEntry> = chosen_indices
        .iter()
        .map(|&idx| entries.get(idx).expect("picker index out of range"))
        .collect();

    println!(
        "Running cargo fmt with {} additional file(s) plus src/lib.rs",
        selected_entries.len()
    );

    let format_result = run_cargo_fmt(&workspace_root, &derive_dir, &selected_entries)?;

    for entry in selected_entries {
        history.insert(&entry.canonical, format_result.ice_found);
    }

    let lib_canonical = derive_dir
        .join("src/lib.rs")
        .canonicalize()
        .wrap_err("failed to canonicalize src/lib.rs for history")?;
    history.insert(&lib_canonical, format_result.ice_found);
    save_history(&history_path, &history)?;

    if format_result.success && !format_result.ice_found {
        println!("cargo fmt finished with no ICE detected.");
    } else {
        println!(
            "cargo fmt failed (ice={} success={}).",
            format_result.ice_found, format_result.success
        );
        println!("Command output:\n{}", format_result.output);
    }

    Ok(())
}

fn load_history(path: &Path) -> Result<History> {
    if !path.exists() {
        return Ok(History::default());
    }
    let file = File::open(path).wrap_err("failed to open history file")?;
    let history = serde_json::from_reader(file).wrap_err("failed to parse history file")?;
    Ok(history)
}

fn save_history(path: &Path, history: &History) -> Result<()> {
    let file = File::create(path).wrap_err("failed to create history file")?;
    serde_json::to_writer_pretty(file, history).wrap_err("failed to write history file")?;
    Ok(())
}

fn collect_files(src_root: &Path) -> Result<Vec<FileEntry>> {
    let mut entries = Vec::new();
    let derive_root = src_root
        .parent()
        .ok_or_else(|| eyre!("failed to determine derive root"))?;
    for entry in WalkDir::new(src_root).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let relative = path
            .strip_prefix(derive_root)
            .wrap_err("failed to compute relative path")?
            .to_path_buf();
        let canonical = path
            .canonicalize()
            .wrap_err_with(|| format!("failed to canonicalize {}", path.display()))?;
        let selectable = relative != PathBuf::from("src/lib.rs");
        entries.push(FileEntry {
            relative,
            canonical,
            selectable,
        });
    }
    entries.sort_by(|a, b| a.relative.cmp(&b.relative));
    Ok(entries)
}

fn build_choices(entries: &[FileEntry], history: &History) -> (Vec<Choice<usize>>, bool) {
    let mut choices = Vec::new();
    for (idx, entry) in entries.iter().enumerate() {
        if !entry.selectable {
            continue;
        }
        let status_label = match history.get(&entry.canonical) {
            Some(true) => "ICE_FOUND",
            Some(false) => "NO_ICE",
            None => "UNKNOWN",
        };
        let key = format!(
            "{} {} ({})",
            status_label,
            entry.canonical.display(),
            entry.relative.display()
        );
        choices.push(Choice { key, value: idx });
    }
    let has_choices = !choices.is_empty();
    (choices, has_choices)
}

fn run_cargo_fmt(
    workspace_root: &Path,
    derive_dir: &Path,
    selected_entries: &[&FileEntry],
) -> Result<FormatResult> {
    let temp_dir = tempdir().wrap_err("failed to create temp directory")?;
    let temp_path = temp_dir.path();

    const WORKSPACE_MANIFEST: &str = r#"[workspace]
resolver = "2"
members = ["derive"]

[workspace.lints.clippy]
doc_markdown = "warn"
manual_let_else = "warn"
match_same_arms = "warn"
redundant_closure_for_method_calls = "warn"
redundant_else = "warn"
semicolon_if_nothing_returned = "warn"
type_complexity = "allow"
undocumented_unsafe_blocks = "warn"
unwrap_or_default = "warn"
needless_lifetimes = "allow"
too_many_arguments = "allow"
nonstandard_macro_braces = "warn"

ptr_as_ptr = "warn"
ptr_cast_constness = "warn"
ref_as_ptr = "warn"

too_long_first_doc_paragraph = "allow"

std_instead_of_core = "warn"
std_instead_of_alloc = "warn"
alloc_instead_of_core = "warn"

allow_attributes = "warn"
allow_attributes_without_reason = "warn"

[workspace.lints.rust]
missing_docs = "warn"
unexpected_cfgs = { level = "warn", check-cfg = ['cfg(docsrs_dep)'] }
unsafe_code = "deny"
unsafe_op_in_unsafe_fn = "warn"
unused_qualifications = "warn"
"#;

    fs::write(temp_path.join("Cargo.toml"), WORKSPACE_MANIFEST)
        .wrap_err("failed to write workspace manifest")?;

    let rustfmt_src = workspace_root.join("rustfmt.toml");
    if rustfmt_src.exists() {
        fs::copy(&rustfmt_src, temp_path.join("rustfmt.toml"))
            .wrap_err("failed to copy rustfmt.toml")?;
    }

    let derive_dst = temp_path.join("derive");
    fs::create_dir_all(&derive_dst).wrap_err("failed to create derive directory")?;

    let cargo_toml_src = derive_dir.join("Cargo.toml");
    let cargo_toml_dst = derive_dst.join("Cargo.toml");
    let cargo_toml =
        fs::read_to_string(&cargo_toml_src).wrap_err("failed to read derive Cargo.toml")?;
    let bevy_macro_utils_path = workspace_root
        .join("crates")
        .join("bevy_macro_utils")
        .canonicalize()
        .wrap_err("failed to canonicalize bevy_macro_utils path")?;
    let bevy_macro_utils_path = normalize_manifest_path(&bevy_macro_utils_path);
    let cargo_toml = cargo_toml.replace(
        "path = \"../../bevy_macro_utils\"",
        &format!("path = \"{}\"", bevy_macro_utils_path),
    );
    fs::write(&cargo_toml_dst, cargo_toml).wrap_err("failed to write derive Cargo.toml")?;

    let temp_src = derive_dst.join("src");
    fs::create_dir_all(&temp_src).wrap_err("failed to create src directory in tempdir")?;

    let lib_src = derive_dir.join("src/lib.rs");
    let lib_dst = temp_src.join("lib.rs");
    fs::copy(&lib_src, &lib_dst).wrap_err("failed to copy src/lib.rs")?;

    let mut copied_relatives: HashSet<PathBuf> = HashSet::new();
    copied_relatives.insert(PathBuf::from("src/lib.rs"));

    for entry in selected_entries {
        let destination = derive_dst.join(&entry.relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).wrap_err("failed to create destination directories")?;
        }
        fs::copy(&entry.canonical, &destination)
            .wrap_err_with(|| format!("failed to copy {}", entry.canonical.display()))?;
        copied_relatives.insert(entry.relative.clone());
    }

    copy_module_dependencies(derive_dir, &derive_dst, &mut copied_relatives)?;

    let output = Command::new("cargo")
        .arg("fmt")
        .current_dir(&derive_dst)
        .output()
        .wrap_err("failed to run cargo fmt")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", stdout, stderr);
    let ice_found = combined.contains("error: the compiler unexpectedly panicked.");
    Ok(FormatResult {
        ice_found,
        success: output.status.success(),
        output: combined,
    })
}

fn normalize_manifest_path(path: &Path) -> String {
    const UNC_PREFIX_BACKSLASH: &str = "\\\\?\\"; // \\?\
    const UNC_PREFIX_SLASH: &str = "//?/";

    let path_str = path.to_string_lossy();
    let mut trimmed: &str = &path_str;
    if let Some(stripped) = path_str
        .strip_prefix(UNC_PREFIX_BACKSLASH)
        .or_else(|| path_str.strip_prefix(UNC_PREFIX_SLASH))
    {
        trimmed = stripped;
    }

    let mut normalized = trimmed.replace('\\', "/");
    // Ensure drive-letter paths are formatted like "D:/..." instead of "D:..."
    if let Some((drive, rest)) = normalized.split_once(':') {
        if !rest.starts_with('/') {
            let rest = rest.trim_start_matches('/');
            normalized = format!("{}:/{}", drive, rest);
        }
    }
    normalized
}

fn copy_module_dependencies(
    derive_src: &Path,
    derive_dst: &Path,
    copied_relatives: &mut HashSet<PathBuf>,
) -> Result<()> {
    let mut queue: Vec<PathBuf> = copied_relatives.iter().cloned().collect();
    while let Some(rel_path) = queue.pop() {
        let src_path = derive_src.join(&rel_path);
        if !src_path.is_file() {
            continue;
        }

        let contents = fs::read_to_string(&src_path)
            .wrap_err_with(|| format!("failed to read {}", src_path.display()))?;
        for module in find_mod_decls(&contents) {
            let parent = rel_path.parent().unwrap_or_else(|| Path::new(""));
            let candidate_files = [
                parent.join(format!("{}.rs", module)),
                parent.join(module).join("mod.rs"),
            ];

            for candidate in candidate_files {
                let source_file = derive_src.join(&candidate);
                if !source_file.exists() {
                    continue;
                }
                if copied_relatives.insert(candidate.clone()) {
                    let dest_file = derive_dst.join(&candidate);
                    if let Some(parent) = dest_file.parent() {
                        fs::create_dir_all(parent)
                            .wrap_err_with(|| format!("failed to create {}", parent.display()))?;
                    }
                    fs::copy(&source_file, &dest_file).wrap_err_with(|| {
                        format!("failed to copy {}", source_file.display())
                    })?;
                    queue.push(candidate);
                }
            }
        }
    }
    Ok(())
}

fn find_mod_decls(src: &str) -> Vec<String> {
    // Matches lines like `mod foo;` or `pub mod foo;` (not inline modules).
    let re = Regex::new(r"(?m)^\s*(?:pub\s+)?mod\s+([A-Za-z0-9_]+)\s*;\s*$").expect("static regex");
    re.captures_iter(src)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::normalize_manifest_path;
    use std::path::Path;

    #[cfg(target_os = "windows")]
    #[test]
    fn normalize_removes_unc_backslash_and_slashes_drive() {
        let raw = "\\\\?\\D:\\bevywinicon\\crates\\bevy_macro_utils";
        let normalized = normalize_manifest_path(Path::new(raw));
        assert_eq!(normalized, "D:/bevywinicon/crates/bevy_macro_utils");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn normalize_removes_unc_slash_prefix() {
        let raw = "//?/D:/bevywinicon/crates/bevy_macro_utils";
        let normalized = normalize_manifest_path(Path::new(raw));
        assert_eq!(normalized, "D:/bevywinicon/crates/bevy_macro_utils");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn normalize_adds_missing_slash_after_drive() {
        let raw = "D:bevywinicon\\crates\\bevy_macro_utils";
        let normalized = normalize_manifest_path(Path::new(raw));
        assert_eq!(normalized, "D:/bevywinicon/crates/bevy_macro_utils");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn normalize_keeps_already_forward_slashes() {
        let raw = "D:/bevywinicon/crates/bevy_macro_utils";
        let normalized = normalize_manifest_path(Path::new(raw));
        assert_eq!(normalized, "D:/bevywinicon/crates/bevy_macro_utils");
    }
}
