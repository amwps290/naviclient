//! 测试专用辅助：仅供 `#[cfg(test)]` 模块使用。

use std::fs;
use std::path::{Path, PathBuf};

/// 递归收集 `src/` 下所有 Rust 源码文件（含测试文件本身）。
pub(crate) fn rust_source_files(root: impl AsRef<Path>) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rust_files(root.as_ref().into(), &mut files);
    files
}

fn collect_rust_files(dir: PathBuf, files: &mut Vec<PathBuf>) {
    let entries =
        fs::read_dir(&dir).unwrap_or_else(|error| panic!("reading {}: {error}", dir.display()));
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
}
