use anyhow::{Context, Result, ensure};
use clap::{Subcommand, ValueEnum};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use syn::visit::Visit;

const APP_ID: &str = "io.github.p4suta.tungsten";
const PERMISSIONS: [&str; 5] = [
    "--share=network",
    "--share=ipc",
    "--socket=wayland",
    "--socket=fallback-x11",
    "--device=dri",
];

#[derive(Clone, Copy, ValueEnum)]
pub enum Architecture {
    #[value(name = "x86_64")]
    X86_64,
    Aarch64,
}

impl Architecture {
    fn name(self) -> &'static str {
        match self {
            Self::X86_64 => "x86_64",
            Self::Aarch64 => "aarch64",
        }
    }
}

#[derive(Subcommand)]
pub enum Action {
    Source,
    Build,
    Run,
    Check,
    Bundle {
        #[arg(long, value_enum)]
        arch: Architecture,
        #[arg(long)]
        resume: Option<PathBuf>,
    },
    Affected {
        #[arg(long)]
        base: String,
        #[arg(long)]
        head: String,
        #[arg(long)]
        output: PathBuf,
    },
}

pub fn run(root: &Path, action: Action) -> Result<()> {
    match action {
        Action::Source => source(root),
        Action::Affected { base, head, output } => affected(root, &base, &head, &output),
        Action::Build => {
            native()?;
            super::run(
                root,
                "cargo",
                &[
                    "build",
                    "--locked",
                    "--package",
                    "tungsten-linux",
                    "--features",
                    "desktop",
                    "--bin",
                    "tungsten-gtk",
                ],
            )
        }
        Action::Run => {
            native()?;
            super::run(
                root,
                "cargo",
                &[
                    "run",
                    "--locked",
                    "--package",
                    "tungsten-linux",
                    "--features",
                    "desktop",
                    "--bin",
                    "tungsten-gtk",
                ],
            )
        }
        Action::Check => check(root),
        Action::Bundle { arch, resume } => bundle(root, arch, resume.as_deref()),
    }
}

fn native() -> Result<()> {
    ensure!(
        std::env::consts::OS == "linux",
        "GTK operations require Linux"
    );
    Ok(())
}

struct SystemDesign {
    violations: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for SystemDesign {
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if item.attrs.iter().any(|attr| {
            attr.path().is_ident("cfg")
                && attr
                    .parse_args::<syn::Path>()
                    .is_ok_and(|path| path.is_ident("test"))
        }) {
            return;
        }
        syn::visit::visit_item_mod(self, item);
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        for segment in &path.segments {
            let name = segment.ident.to_string();
            if [
                "CssProvider",
                "WebView",
                "WebKitWebView",
                "FontDescription",
                "AttrSize",
                "AttrFontDesc",
                "AttrForeground",
                "AttrBackground",
            ]
            .contains(&name.as_str())
            {
                self.violations.insert(name);
            }
        }
        syn::visit::visit_path(self, path);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let name = call.method.to_string();
        if [
            "set_opacity",
            "opacity",
            "set_font_description",
            "set_css",
            "load_from_data",
            "set_color_scheme",
            "set_gtk_font_name",
            "set_gtk_theme_name",
            "set_gtk_application_prefer_dark_theme",
            "set_font",
            "set_size_points",
            "set_size",
            "size_points",
            "foreground_rgba",
            "background_rgba",
        ]
        .contains(&name.as_str())
        {
            self.violations.insert(name);
        }
        syn::visit::visit_expr_method_call(self, call);
    }
}

fn design(source: &str) -> Result<()> {
    let syntax = syn::parse_file(source)?;
    let mut check = SystemDesign {
        violations: BTreeSet::new(),
    };
    check.visit_file(&syntax);
    ensure!(
        check.violations.is_empty(),
        "use system GTK styling: {:?}",
        check.violations
    );
    Ok(())
}

fn manifest(value: &Value) -> Result<()> {
    ensure!(
        value["app-id"] == APP_ID && value["command"] == "tungsten-gtk",
        "Flatpak identity differs"
    );
    ensure!(
        value["runtime"] == "org.gnome.Platform"
            && value["sdk"] == "org.gnome.Sdk"
            && value["runtime-version"] == "51",
        "Flatpak runtime differs"
    );
    let permissions = value["finish-args"]
        .as_array()
        .context("Flatpak permissions")?;
    let admitted: BTreeSet<_> = PERMISSIONS.iter().copied().collect();
    let actual: BTreeSet<_> = permissions.iter().filter_map(Value::as_str).collect();
    ensure!(
        actual == admitted && permissions.len() == admitted.len(),
        "Flatpak permissions exceed the desktop contract"
    );
    ensure!(
        value["build-options"]["env"]["CARGO_NET_OFFLINE"] == "true",
        "Flatpak Cargo builds must be offline"
    );
    let modules = value["modules"].as_array().context("Flatpak modules")?;
    ensure!(
        modules.len() == 1 && modules[0]["name"] == "tungsten",
        "unexpected Flatpak module"
    );
    let commands = modules[0]["build-commands"]
        .as_array()
        .context("build commands")?;
    ensure!(commands.iter().any(|command| command.as_str().is_some_and(|command|
        command.contains("cargo build --release --locked --offline --package tungsten-linux --features desktop --bin tungsten-gtk"))), "build must select locked desktop dependencies");
    Ok(())
}

fn desktop(source: &str) -> Result<()> {
    for required in [
        "[Desktop Entry]",
        "Type=Application",
        "Name=Tungsten",
        "Exec=tungsten-gtk",
        "Icon=io.github.p4suta.tungsten",
        "Terminal=false",
        "Categories=GNOME;GTK;Utility;TextEditor;",
    ] {
        ensure!(
            source.lines().filter(|line| *line == required).count() == 1,
            "missing or repeated desktop field {required}"
        );
    }
    Ok(())
}

fn metadata(source: &str) -> Result<()> {
    let document = roxmltree::Document::parse(source)?;
    let root = document.root_element();
    ensure!(
        root.tag_name().name() == "component"
            && root.attribute("type") == Some("desktop-application"),
        "metadata component differs"
    );
    for (element, expected) in [
        ("id", APP_ID),
        ("launchable", "io.github.p4suta.tungsten.desktop"),
        ("name", "Tungsten"),
    ] {
        let values: Vec<_> = root
            .children()
            .filter(|node| node.has_tag_name(element))
            .collect();
        ensure!(
            values.len() == 1 && values[0].text() == Some(expected),
            "metadata identity differs: {element}"
        );
    }
    Ok(())
}

pub fn source(root: &Path) -> Result<()> {
    let mut files = Vec::new();
    super::collect(&root.join("linux/src"), "rs", &mut files)?;
    for path in files {
        design(&fs::read_to_string(&path)?).with_context(|| path.clone())?;
    }
    let model: toml::Value =
        toml::from_str(&fs::read_to_string(root.join("linux/model/Cargo.toml"))?)?;
    ensure!(
        model.get("dependencies").is_none(),
        "the pure model must have no dependencies"
    );
    let model_lib = syn::parse_file(&fs::read_to_string(root.join("linux/model/src/lib.rs"))?)?;
    ensure!(
        model_lib
            .attrs
            .iter()
            .any(|attribute| attribute.path().is_ident("no_std")),
        "the pure model must exclude std"
    );
    manifest(&serde_json::from_slice(&fs::read(
        root.join("linux/flatpak/io.github.p4suta.tungsten.json"),
    )?)?)?;
    desktop(&fs::read_to_string(
        root.join("linux/data/io.github.p4suta.tungsten.desktop"),
    )?)?;
    metadata(&fs::read_to_string(
        root.join("linux/data/io.github.p4suta.tungsten.metainfo.xml"),
    )?)?;
    let icon = fs::read_to_string(root.join("linux/data/io.github.p4suta.tungsten.svg"))?;
    ensure!(
        roxmltree::Document::parse(&icon)?
            .root_element()
            .has_tag_name("svg"),
        "invalid application icon"
    );
    eprintln!("phase=linux-source outcome=passed");
    Ok(())
}

fn check(root: &Path) -> Result<()> {
    native()?;
    source(root)?;
    super::run(
        root,
        "cargo",
        &[
            "clippy",
            "--locked",
            "--package",
            "tungsten-linux",
            "--package",
            "tungsten-linux-model",
            "--all-targets",
            "--features",
            "desktop",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    super::run(
        root,
        "cargo",
        &[
            "test",
            "--locked",
            "--package",
            "tungsten-linux",
            "--package",
            "tungsten-linux-model",
        ],
    )?;
    super::run(
        root,
        "desktop-file-validate",
        &["linux/data/io.github.p4suta.tungsten.desktop"],
    )?;
    super::run(
        root,
        "appstreamcli",
        &[
            "validate",
            "--no-net",
            "linux/data/io.github.p4suta.tungsten.metainfo.xml",
        ],
    )?;
    let data = tempfile::tempdir()?;
    let configuration = tempfile::tempdir()?;
    let cache = tempfile::tempdir()?;
    let screenshots = root.join("target/linux/screenshots");
    fs::create_dir_all(&screenshots)?;
    for (contrast, label) in [("0", "standard"), ("1", "high-contrast")] {
        let captures = screenshots.join(label);
        fs::create_dir_all(&captures)?;
        super::run_env(
            root,
            "xvfb-run",
            &[
                "--auto-servernum",
                "--server-args=-screen 0 1600x1200x24",
                "dbus-run-session",
                "--",
                "cargo",
                "test",
                "--locked",
                "--package",
                "tungsten-linux",
                "--features",
                "desktop",
                "--bin",
                "tungsten-gtk",
                "--",
                "--test-threads=1",
            ],
            &[
                ("GSK_RENDERER", "cairo"),
                ("ADW_DEBUG_HIGH_CONTRAST", contrast),
                (
                    "XDG_DATA_HOME",
                    data.path().to_str().context("isolated application data")?,
                ),
                (
                    "XDG_CONFIG_HOME",
                    configuration
                        .path()
                        .to_str()
                        .context("isolated configuration")?,
                ),
                (
                    "XDG_CACHE_HOME",
                    cache.path().to_str().context("isolated cache")?,
                ),
                (
                    "TUNGSTEN_SCREENSHOT_DIRECTORY",
                    captures.to_str().context("screenshot directory")?,
                ),
            ],
        )?;
    }
    Ok(())
}

fn relevant(path: &str) -> bool {
    ["linux/", "src/", "xtask/", ".github/workflows/"]
        .iter()
        .any(|prefix| path.starts_with(prefix))
        || [
            "Cargo.toml",
            "Cargo.lock",
            "mise.toml",
            "justfile",
            "deny.toml",
            ".cargo/config.toml",
        ]
        .contains(&path)
}

fn affected(root: &Path, base: &str, head: &str, output: &Path) -> Result<()> {
    for revision in [base, head] {
        ensure!(
            revision.len() == 40 && revision.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "scope requires exact commit identities"
        );
    }
    let paths = super::output(root, "git", &["diff", "--name-only", base, head, "--"])?;
    let affected = paths.lines().any(relevant);
    writeln!(
        fs::OpenOptions::new().append(true).open(output)?,
        "affected={affected}"
    )?;
    eprintln!("phase=linux-scope affected={affected}");
    Ok(())
}

fn copy_sources(root: &Path, destination: &Path) -> Result<()> {
    for directory in ["src", "linux", "xtask"] {
        let path = destination.join(directory);
        if path.exists() {
            ensure!(
                fs::symlink_metadata(&path)?.is_dir(),
                "invalid generated source directory"
            );
            fs::remove_dir_all(path)?;
        }
    }
    let files = super::output(
        root,
        "git",
        &["ls-files", "--cached", "--others", "--exclude-standard"],
    )?;
    for path in files.lines().filter(|path| {
        ["src/", "linux/", "xtask/"]
            .iter()
            .any(|prefix| path.starts_with(prefix))
            || ["Cargo.toml", "Cargo.lock", "LICENSE-APACHE", "LICENSE-MIT"].contains(path)
    }) {
        let source = root.join(path);
        ensure!(
            fs::symlink_metadata(&source)?.is_file(),
            "bundle source must be a regular file: {path}"
        );
        let target = destination.join(path);
        fs::create_dir_all(target.parent().context("source directory")?)?;
        fs::copy(source, target)?;
    }
    Ok(())
}

fn license_files(root: &Path, directory: &Path, destination: &Path) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        ensure!(
            !kind.is_symlink(),
            "vendored license sources must not contain symbolic links"
        );
        if kind.is_dir() {
            license_files(root, &path, destination)?;
        } else if kind.is_file() {
            let name = entry.file_name().to_string_lossy().to_ascii_uppercase();
            if ["LICENSE", "LICENCE", "COPYING", "COPYRIGHT", "NOTICE"]
                .iter()
                .any(|prefix| name.starts_with(prefix))
            {
                let target = destination.join(path.strip_prefix(root)?);
                fs::create_dir_all(target.parent().context("license directory")?)?;
                fs::copy(path, target)?;
            }
        }
    }
    Ok(())
}

fn licenses(vendor: &Path, destination: &Path) -> Result<()> {
    if destination.exists() {
        fs::remove_dir_all(destination)?;
    }
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(vendor)? {
        let entry = entry?;
        ensure!(entry.file_type()?.is_dir(), "unexpected vendored package");
        let root = entry.path();
        let target = destination.join(entry.file_name());
        let manifest = fs::read_to_string(root.join("Cargo.toml"))?;
        let package: toml::Value = toml::from_str(&manifest)?;
        fs::create_dir_all(&target)?;
        fs::write(target.join("Cargo.toml"), manifest)?;
        license_files(&root, &root, &target)?;
        if let Some(file) = package["package"]
            .get("license-file")
            .and_then(toml::Value::as_str)
        {
            let path = Path::new(file);
            ensure!(
                path.components()
                    .all(|part| matches!(part, std::path::Component::Normal(_))),
                "license file escapes its package"
            );
            let source = root.join(path);
            ensure!(
                fs::symlink_metadata(&source)?.is_file(),
                "invalid package license file"
            );
            let path = target.join(path);
            fs::create_dir_all(path.parent().context("license file directory")?)?;
            fs::copy(source, path)?;
        }
    }
    Ok(())
}

fn checksum(path: &Path) -> Result<String> {
    let mut digest = Sha256::new();
    let mut file = fs::File::open(path)?;
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn rust_module(root: &Path, staging: &Path, arch: Architecture) -> Result<Value> {
    let mise: toml::Value = toml::from_str(&fs::read_to_string(root.join("mise.toml"))?)?;
    let version = mise["tools"]["rust"]["version"]
        .as_str()
        .context("pinned Rust version")?;
    ensure!(
        version.split('.').count() == 3
            && version
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'.'),
        "Rust version must be exact"
    );
    let target = format!("{}-unknown-linux-gnu", arch.name());
    let mut sources = Vec::new();
    let mut commands = Vec::new();
    for component in ["rustc", "cargo", "rust-std"] {
        let file = format!("{component}-{version}-{target}.tar.xz");
        let archive = staging.join(&file);
        let hash_file = staging.join(format!("{file}.sha256"));
        for (url, path) in [
            (
                format!("https://static.rust-lang.org/dist/{file}"),
                &archive,
            ),
            (
                format!("https://static.rust-lang.org/dist/{file}.sha256"),
                &hash_file,
            ),
        ] {
            if path.exists() {
                continue;
            }
            super::run(
                root,
                "curl",
                &[
                    "--fail",
                    "--silent",
                    "--show-error",
                    "--location",
                    "--output",
                    path.to_str().context("Rust source path")?,
                    &url,
                ],
            )?;
        }
        let hash = checksum(&archive)?;
        ensure!(
            fs::read_to_string(hash_file)?.split_whitespace().next() == Some(&hash),
            "Rust archive checksum differs"
        );
        sources
            .push(json!({"type": "archive", "path": archive, "sha256": hash, "dest": component}));
        commands.push(format!(
            "./{component}/install.sh --prefix=/app --disable-ldconfig"
        ));
    }
    Ok(
        json!({"name": "rust-toolchain", "buildsystem": "simple", "build-options": {"no-debuginfo": true, "strip": false}, "build-commands": commands, "sources": sources}),
    )
}

fn bundle(root: &Path, arch: Architecture, resume: Option<&Path>) -> Result<()> {
    native()?;
    source(root)?;
    let directory = root.join("target/linux");
    fs::create_dir_all(&directory)?;
    let staging = match resume {
        Some(path) => {
            let path = path.canonicalize()?;
            ensure!(
                path.parent() == Some(directory.canonicalize()?.as_path())
                    && path.file_name().is_some_and(|name| name
                        .to_string_lossy()
                        .starts_with(&format!("flatpak-{}-", arch.name()))),
                "resume requires this architecture's generated staging directory"
            );
            path
        }
        None => tempfile::Builder::new()
            .prefix(&format!("flatpak-{}-", arch.name()))
            .tempdir_in(&directory)?
            .keep(),
    };
    let source = staging.join("source");
    copy_sources(root, &source)?;
    let vendor = source.join("vendor");
    let config = super::output(
        root,
        "cargo",
        &[
            "vendor",
            "--locked",
            "--versioned-dirs",
            vendor.to_str().context("vendor path")?,
        ],
    )?;
    fs::create_dir_all(source.join(".cargo"))?;
    let relative = config.replace(vendor.to_str().context("vendor path")?, "vendor");
    fs::write(source.join(".cargo/config.toml"), relative)?;
    licenses(&vendor, &source.join("licenses"))?;
    let mut value: Value = serde_json::from_slice(&fs::read(
        root.join("linux/flatpak/io.github.p4suta.tungsten.json"),
    )?)?;
    value["build-options"]["prepend-path"] = json!("/app/bin");
    value["build-options"]["env"]["RUSTC"] = json!("/app/bin/rustc");
    value["build-options"]["build-args"] = json!(["--unshare=network"]);
    value["cleanup"] = json!([
        "/bin/cargo",
        "/bin/rust*",
        "/lib/rustlib",
        "/lib/libLLVM*",
        "/lib/librustc*",
        "/libexec/rust*",
        "/share/doc/rust*",
        "/share/man"
    ]);
    value["modules"]
        .as_array_mut()
        .context("Flatpak modules")?
        .insert(0, rust_module(root, &staging, arch)?);
    let manifest = staging.join(format!("{APP_ID}.json"));
    fs::write(&manifest, serde_json::to_vec_pretty(&value)?)?;
    let build = staging.join("build");
    let repository = staging.join("repo");
    let repo_arg = format!("--repo={}", repository.display());
    if !repository.join("config").exists() {
        super::run(root, "ostree", &[&repo_arg, "init", "--mode=archive-z2"])?;
    }
    super::run(
        root,
        "ostree",
        &[
            &repo_arg,
            "config",
            "set",
            "core.min-free-space-size",
            "1GB",
        ],
    )?;
    let arch_arg = format!("--arch={}", arch.name());
    super::run(
        &staging,
        "flatpak-builder",
        &[
            "--user",
            "--disable-rofiles-fuse",
            "--force-clean",
            &arch_arg,
            "--repo",
            repository.to_str().context("Flatpak repository")?,
            build.to_str().context("Flatpak build directory")?,
            manifest.to_str().context("Flatpak manifest")?,
        ],
    )?;
    fs::create_dir_all(root.join("dist/linux"))?;
    let artifact = root.join(format!("dist/linux/{APP_ID}-{}.flatpak", arch.name()));
    super::run(
        root,
        "flatpak",
        &[
            "build-bundle",
            &arch_arg,
            repository.to_str().context("Flatpak repository")?,
            artifact.to_str().context("bundle path")?,
            APP_ID,
            "master",
        ],
    )?;
    super::run(
        root,
        "flatpak",
        &[
            "--user",
            "install",
            "--assumeyes",
            "--noninteractive",
            "--reinstall",
            artifact.to_str().context("bundle path")?,
        ],
    )?;
    super::run(
        root,
        "xvfb-run",
        &[
            "--auto-servernum",
            "dbus-run-session",
            "--",
            "flatpak",
            "run",
            "--user",
            &arch_arg,
            APP_ID,
            "--smoke-test",
        ],
    )?;
    eprintln!(
        "phase=linux-bundle outcome=passed architecture={} artifact={}",
        arch.name(),
        artifact.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_design_accepts_native_widgets_and_rejects_custom_styles() {
        assert!(design("fn main() { let _ = gtk::TextView::new(); }").is_ok());
        for source in [
            "fn main() { gtk::CssProvider::new(); }",
            "fn main() { view.set_opacity(0.5); }",
            "fn main() { gtk::WebView::new(); }",
            "fn main() { view.set_font_description(font); }",
            "fn main() { style.set_color_scheme(scheme); }",
            "fn main() { settings.set_gtk_font_name(Some(\"Sans 12\")); }",
            "fn main() { gtk::pango::AttrSize::new(12000); }",
            "fn main() { tag.foreground_rgba(color); }",
        ] {
            assert!(design(source).is_err(), "{source}");
        }
    }

    #[test]
    fn dependency_notices_preserve_license_files_and_refuse_escaping_paths() {
        let directory = tempfile::tempdir().unwrap();
        let vendor = directory.path().join("vendor");
        let package = vendor.join("example-1.0.0");
        fs::create_dir_all(package.join("legal")).unwrap();
        fs::write(
            package.join("Cargo.toml"),
            "[package]\nname = 'example'\nversion = '1.0.0'\nlicense-file = 'legal/terms.txt'\n",
        )
        .unwrap();
        fs::write(package.join("legal/terms.txt"), "Required terms").unwrap();
        fs::write(package.join("NOTICE"), "Attribution").unwrap();
        let destination = directory.path().join("licenses");
        licenses(&vendor, &destination).unwrap();
        assert_eq!(
            fs::read_to_string(destination.join("example-1.0.0/legal/terms.txt")).unwrap(),
            "Required terms"
        );
        assert!(destination.join("example-1.0.0/NOTICE").is_file());
        fs::write(
            package.join("Cargo.toml"),
            "[package]\nlicense-file = '../outside.txt'\n",
        )
        .unwrap();
        assert!(licenses(&vendor, &destination).is_err());
    }

    #[test]
    fn manifest_rejects_excess_permissions_wrong_identity_and_online_builds() {
        let valid: Value = serde_json::from_str(include_str!(
            "../../linux/flatpak/io.github.p4suta.tungsten.json"
        ))
        .unwrap();
        manifest(&valid).unwrap();
        for forbidden in [
            "--filesystem=host",
            "--socket=session-bus",
            "--device=all",
            "--socket=x11",
        ] {
            let mut invalid = valid.clone();
            invalid["finish-args"]
                .as_array_mut()
                .unwrap()
                .push(json!(forbidden));
            assert!(manifest(&invalid).is_err());
        }
        for (path, replacement) in [("app-id", "other.example.app"), ("runtime-version", "50")] {
            let mut invalid = valid.clone();
            invalid[path] = json!(replacement);
            assert!(manifest(&invalid).is_err());
        }
        let mut invalid = valid;
        invalid["build-options"]["env"]["CARGO_NET_OFFLINE"] = json!("false");
        assert!(manifest(&invalid).is_err());
    }

    #[test]
    fn desktop_and_appstream_identity_checks_reject_mismatched_fixtures() {
        let entry = include_str!("../../linux/data/io.github.p4suta.tungsten.desktop");
        desktop(entry).unwrap();
        assert!(desktop(&entry.replace("Exec=tungsten-gtk", "Exec=other")).is_err());
        let xml = include_str!("../../linux/data/io.github.p4suta.tungsten.metainfo.xml");
        metadata(xml).unwrap();
        assert!(metadata(&xml.replace(APP_ID, "io.example.other")).is_err());
        assert!(metadata("<component>").is_err());
    }

    #[test]
    fn native_scope_includes_shared_contracts_and_excludes_browser_styling() {
        for path in [
            "linux/src/ui.rs",
            "src/domain.rs",
            "xtask/src/linux.rs",
            "Cargo.lock",
            ".github/workflows/ci.yml",
        ] {
            assert!(relevant(path));
        }
        for path in [
            "README.md",
            "docs/running.md",
            "ui/style.css",
            "apple/Sources/TungstenKit/RootView.swift",
        ] {
            assert!(!relevant(path));
        }
    }
}
