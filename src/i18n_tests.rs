//! 界面语言目录的守卫测试。
//!
//! 三个约束，任何一条不满足都说明有用户会看到原文或坏掉的占位符：
//! 1. 源码里每个 `t!("…")` 的键都出现在 en / zh-CN / zh-TW 三个目录里；
//! 2. 目录里没有源码已不再使用的死键（防止文案漂移后残留误导翻译）；
//! 3. 同一个键在各语言里的 `%{name}` 占位符集合完全一致——否则翻译会
//!    把参数吞掉或拼出 `%{count}` 字样（rust-i18n 只认 `%{name}`）。

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const LOCALES: [&str; 3] = ["en", "zh-CN", "zh-TW"];

/// 从 `src/` 下所有 Rust 源码里收集 `t!("…")` 字面量键。
fn source_keys() -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    let mut files = Vec::new();
    collect_rust_files(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    for path in files {
        // 本文件自身既包含 `t!("` 的扫描样本，也包含文档里的示例，
        // 跳过它，否则守卫会把自己的实现细节当成文案键。
        if path.ends_with("i18n_tests.rs") {
            continue;
        }
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
        for key in keys_in_source(&text) {
            keys.insert(key);
        }
    }
    keys
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

/// 极简 Rust 词法扫描：找 `t!(` 后紧跟的字符串字面量并反转义。
/// 不处理 `t!(variable)` 的动态键（那些键必须同时以字面量形式出现，
/// 否则目录守卫就看不到它们）。
fn keys_in_source(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut keys = Vec::new();
    let mut index = 0;
    while index + 2 < bytes.len() {
        if &bytes[index..index + 3] != b"t!(" {
            index += 1;
            continue;
        }
        let previous = index.checked_sub(1).map(|i| bytes[i]).unwrap_or(b' ');
        if previous.is_ascii_alphanumeric() || previous == b'_' || previous == b'!' {
            index += 3;
            continue;
        }
        let mut cursor = index + 3;
        while cursor < bytes.len() && (bytes[cursor] as char).is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= bytes.len() || bytes[cursor] != b'"' {
            index += 3;
            continue;
        }
        cursor += 1;
        let mut literal = String::new();
        while cursor < bytes.len() {
            match bytes[cursor] {
                b'\\' if cursor + 1 < bytes.len() => {
                    let escaped = match bytes[cursor + 1] {
                        b'n' => '\n',
                        b't' => '\t',
                        b'r' => '\r',
                        b'0' => '\0',
                        other => other as char,
                    };
                    literal.push(escaped);
                    cursor += 2;
                }
                b'"' => break,
                other => {
                    // 源码是 UTF-8；多字节字符直接按字符推进。
                    let ch = text[cursor..].chars().next().expect("char boundary");
                    literal.push(ch);
                    cursor += ch.len_utf8();
                    let _ = other;
                }
            }
        }
        keys.push(literal);
        index = cursor + 1;
    }
    keys
}

/// 读取一个语言目录。文件是我们严格生成的扁平 `"键": "值"` 每行一条，
/// 因此每行本身就是一个合法 JSON 对象体，直接用 serde_json 解析，
/// 转义语义（`\n`、引号）与 rust-i18n 的 YAML 读取完全一致。
fn locale_catalog(locale: &str) -> BTreeMap<String, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("locales")
        .join(format!("{locale}.yml"));
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
    let mut catalog = BTreeMap::new();
    for (line_no, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let wrapped = format!("{{{line}}}");
        let mut parsed: BTreeMap<String, String> =
            serde_json::from_str(&wrapped).unwrap_or_else(|error| {
                panic!(
                    "{}:{}: not a flat \"key\": \"value\" line: {error}",
                    path.display(),
                    line_no + 1
                )
            });
        let (key, value) = parsed.pop_first().expect("one entry per line");
        assert!(
            parsed.is_empty(),
            "{}:{}: more than one entry",
            path.display(),
            line_no + 1
        );
        assert!(
            catalog.insert(key.clone(), value).is_none(),
            "{}: duplicate key {key:?}",
            path.display()
        );
    }
    catalog
}

/// 键里出现的 `%{name}` 占位符名（排序去重）。
fn placeholders(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let bytes = text.as_bytes();
    let mut index = 0;
    while index + 1 < bytes.len() {
        if bytes[index] == b'%' && bytes[index + 1] == b'{' {
            let start = index + 2;
            if let Some(offset) = text[start..].find('}') {
                let name = &text[start..start + offset];
                if !name.is_empty()
                    && name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                {
                    found.insert(name.to_string());
                }
                index = start + offset + 1;
                continue;
            }
        }
        index += 1;
    }
    found
}

#[test]
fn every_source_key_has_a_translation() {
    let keys = source_keys();
    assert!(
        keys.len() > 300,
        "只扫到 {} 个 t!() 键，扫描器大概失效了",
        keys.len()
    );
    for locale in LOCALES {
        let catalog = locale_catalog(locale);
        let missing: Vec<&String> = keys
            .iter()
            .filter(|key| !catalog.contains_key(*key))
            .collect();
        assert!(
            missing.is_empty(),
            "{locale}.yml 缺少 {} 条文案，例如：{:?}",
            missing.len(),
            &missing[..missing.len().min(8)]
        );
    }
}

#[test]
fn catalogs_have_no_dead_keys() {
    let keys = source_keys();
    for locale in LOCALES {
        let catalog = locale_catalog(locale);
        let dead: Vec<&String> = catalog.keys().filter(|key| !keys.contains(*key)).collect();
        assert!(
            dead.is_empty(),
            "{locale}.yml 有 {} 条源码已不再使用的文案，例如：{:?}",
            dead.len(),
            &dead[..dead.len().min(8)]
        );
    }
}

#[test]
fn translations_keep_the_same_placeholders() {
    let expected = locale_catalog("en");
    for locale in LOCALES.iter().filter(|locale| **locale != "en") {
        let catalog = locale_catalog(locale);
        for (key, value) in &catalog {
            let source = placeholders(key);
            let translated = placeholders(value);
            assert_eq!(
                source, translated,
                "{locale}.yml 的 {key:?} 占位符不一致：{translated:?}（应为 {source:?}）"
            );
            let english = expected
                .get(key)
                .unwrap_or_else(|| panic!("en.yml 缺少 {key:?}"));
            assert_eq!(
                placeholders(english),
                source,
                "en.yml 的 {key:?} 占位符与键不一致"
            );
        }
    }
}

#[test]
fn translations_fill_in_placeholders() {
    // rust-i18n 只替换 `%{name}`；写成 `{name}` 会原样显示给用户。
    assert_eq!(
        rust_i18n::t!("%{count} selected", locale = "zh-CN", count = 3).as_ref(),
        "已选 3 首"
    );
    assert_eq!(
        rust_i18n::t!("%{count} selected", locale = "zh-TW", count = 3).as_ref(),
        "已選 3 首"
    );
    assert_eq!(
        rust_i18n::t!("%{count} selected", locale = "en", count = 3).as_ref(),
        "3 selected"
    );
    // 多行文案保留换行位置。
    assert!(
        rust_i18n::t!(
            "%{title}\nOpen now playing",
            locale = "zh-CN",
            title = "Song"
        )
        .contains('\n'),
        "换行占位符被吃掉了"
    );
    // 排版引号、多个占位符与句末标点要经过 YAML 与插值两层不被打乱。
    assert_eq!(
        rust_i18n::t!(
            "Delete “%{name}” from Navidrome? The music files will be kept.",
            locale = "zh-CN",
            name = "Mix"
        )
        .as_ref(),
        "从 Navidrome 删除“Mix”？音乐文件会保留。"
    );
    // 占位符在句首/句中都要替换，不能残留 `%{count}`。
    let tracks = rust_i18n::t!("%{count} tracks", locale = "zh-TW", count = 12);
    assert_eq!(tracks.as_ref(), "12 首歌曲");
    assert!(!tracks.contains("%{"), "占位符没有被替换：{tracks}");
}

#[test]
fn every_language_is_selectable_at_runtime() {
    // 切换语言后立刻生效：同一个键在不同 locale 下取到不同文案。
    let zh = rust_i18n::t!("Language", locale = "zh-CN").to_string();
    let tw = rust_i18n::t!("Language", locale = "zh-TW").to_string();
    let en = rust_i18n::t!("Language", locale = "en").to_string();
    assert_eq!(
        (zh.as_str(), tw.as_str(), en.as_str()),
        ("语言", "語言", "Language")
    );
}
