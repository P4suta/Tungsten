use anyhow::{Context, Result, ensure};
use clap::{Subcommand, ValueEnum};
use std::{
    fs,
    io::Write,
    net::SocketAddr,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Subcommand)]
pub enum Action {
    Source,
    Icons,
    Affected {
        #[arg(long)]
        base: String,
        #[arg(long)]
        head: String,
        #[arg(long)]
        output: PathBuf,
    },
    Build {
        #[arg(value_enum, default_value = "macos")]
        platform: Platform,
        #[arg(long)]
        release: bool,
    },
    Check,
    Ui {
        #[arg(value_enum, default_value = "ios")]
        platform: Platform,
        #[arg(long)]
        tour: bool,
        #[arg(long, value_enum)]
        only: Option<UiScenario>,
        #[arg(long, value_enum)]
        appearance: Option<Appearance>,
        #[arg(long, value_enum)]
        text_size: Option<TextSize>,
        /// Acknowledge explicitly authorized access to the test desktop.
        #[arg(long)]
        allow_desktop_control: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Platform {
    Macos,
    Ios,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum UiScenario {
    Writing,
    Workspace,
    EmptyCollections,
    Tour,
}

impl UiScenario {
    fn method(self) -> &'static str {
        match self {
            Self::Writing => "testWritingPersistsAcrossRelaunch",
            Self::Workspace => "testWorkspaceFormAccessibility",
            Self::EmptyCollections => "testEmptyCollectionsAccessibility",
            Self::Tour => "testNativeScreenTour",
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Appearance {
    Light,
    Dark,
}

impl Appearance {
    fn name(self) -> &'static str {
        match self {
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
pub enum TextSize {
    Standard,
    Enlarged,
}

impl TextSize {
    fn enlarged(self) -> bool {
        matches!(self, Self::Enlarged)
    }
}

fn tokens(source: &str) -> Vec<String> {
    let mut chars = source.chars().peekable();
    let mut result = Vec::new();
    while let Some(ch) = chars.next() {
        if ch == '/' && chars.peek() == Some(&'/') {
            chars.by_ref().take_while(|ch| *ch != '\n').for_each(drop);
        } else if ch == '/' && chars.peek() == Some(&'*') {
            chars.next();
            let mut depth = 1;
            while let Some(ch) = chars.next() {
                if ch == '/' && chars.peek() == Some(&'*') {
                    chars.next();
                    depth += 1;
                } else if ch == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
        } else if ch == '"' {
            while let Some(ch) = chars.next() {
                if ch == '\\' {
                    chars.next();
                } else if ch == '"' {
                    break;
                }
            }
            result.push("literal".to_owned());
        } else if ch.is_alphanumeric() || ch == '_' {
            let mut word = ch.to_string();
            while chars
                .peek()
                .is_some_and(|ch| ch.is_alphanumeric() || *ch == '_')
            {
                if let Some(ch) = chars.next() {
                    word.push(ch);
                }
            }
            result.push(word);
        } else if !ch.is_whitespace() {
            result.push(ch.to_string());
        }
    }
    result
}

fn violations(source: &str) -> Vec<&'static str> {
    let tokens = tokens(source);
    let mut result = Vec::new();
    for token in &tokens {
        if [
            "toolbarBackground",
            "presentationBackground",
            "preferredColorScheme",
            "glassEffect",
            "glassEffectID",
            "glassEffectUnion",
            "GlassEffectContainer",
            "lineLimit",
            "minimumScaleFactor",
            "truncationMode",
            "WKWebView",
            "UIWebView",
            "SFSafariViewController",
            "NSColorPanel",
        ]
        .contains(&token.as_str())
        {
            result.push("use standard native components and adaptive content");
        }
    }
    for pair in tokens.windows(2) {
        if pair[0] == "Color" && pair[1] == "(" {
            result.push("use semantic system colors");
        }
        if ["try", "as"].contains(&pair[0].as_str()) && pair[1] == "!" {
            result.push("handle invalid inputs without a trap");
        }
    }
    for window in tokens.windows(3) {
        if window[0] == "font" && window[1] == "(" && window[2] != "." {
            result.push("use system text styles");
        }
    }
    for window in tokens.windows(4) {
        if window[0] == "foregroundStyle" && window[1] == "(" && window[2] == "." {
            result.push("use explicit semantic colors for adaptive contrast");
        }
        if window[0] == "system" && window[1] == "(" && window[2] == "size" && window[3] == ":" {
            result.push("use system text styles without fixed point sizes");
        }
    }
    for window in tokens.windows(2) {
        if window[0] == "."
            && [
                "red", "blue", "green", "yellow", "orange", "pink", "purple", "mint", "teal",
                "cyan", "indigo", "brown", "gray", "white", "black",
            ]
            .contains(&window[1].as_str())
        {
            result.push("use semantic system colors");
        }
    }
    result.sort_unstable();
    result.dedup();
    result
}

pub fn source(root: &Path) -> Result<()> {
    let mut paths = Vec::new();
    super::collect(&root.join("apple/Sources"), "swift", &mut paths)?;
    super::collect(&root.join("apple/App"), "swift", &mut paths)?;
    ensure!(!paths.is_empty(), "missing native application source");
    for path in paths {
        let text = fs::read_to_string(&path)?;
        ensure!(
            violations(&text).is_empty(),
            "{path}: {:?}",
            violations(&text)
        );
    }
    let release = fs::read_to_string(root.join("apple/App/Info-Release.plist"))?;
    ensure!(
        !release.contains("NSAllows"),
        "distribution transport security must remain enabled"
    );
    ensure!(
        !release.contains("UIDesignRequiresCompatibility"),
        "the system design must remain enabled"
    );
    eprintln!("phase=apple-source outcome=passed");
    Ok(())
}

pub fn run(root: &Path, action: Action) -> Result<()> {
    match action {
        Action::Source => source(root),
        Action::Icons => icons(root),
        Action::Affected { base, head, output } => affected(root, &base, &head, &output),
        Action::Build { platform, release } => build(root, platform, release),
        Action::Check => check(root),
        Action::Ui {
            platform,
            tour,
            only,
            appearance,
            text_size,
            allow_desktop_control,
        } => {
            ensure!(
                allow_desktop_control,
                "native UI checks control the desktop; get explicit owner permission and coordinate access before passing --allow-desktop-control"
            );
            ui(root, platform, tour, only, appearance, text_size)
        }
    }
}

fn generate(root: &Path) -> Result<()> {
    ensure!(
        cfg!(target_os = "macos"),
        "native builds require macOS with Xcode 27.2"
    );
    let version = super::output(root, "xcodebuild", &["-version"])?;
    ensure!(
        version.starts_with("Xcode 27.2\n"),
        "Xcode 27.2 is required: {version}"
    );
    let resolved = super::output(
        root,
        "xcodegen",
        &[
            "dump",
            "--spec",
            "apple/project.yml",
            "--type",
            "parsed-json",
        ],
    )?;
    check_standard_schemes(&serde_json::from_str(&resolved)?)?;
    super::run(
        root,
        "xcodegen",
        &["generate", "--spec", "apple/project.yml"],
    )
}

fn check_standard_schemes(spec: &serde_json::Value) -> Result<()> {
    for application in ["TungstenMac", "TungstenIOS"] {
        let tests = spec
            .pointer(&format!("/schemes/{application}/test/targets"))
            .and_then(serde_json::Value::as_array)
            .with_context(|| format!("missing standard test action for {application}"))?;
        ensure!(
            !tests.is_empty()
                && tests.iter().all(|test| {
                    test.as_str().is_some_and(|name| {
                        spec["targets"][name]["type"].as_str() == Some("bundle.unit-test")
                    })
                }),
            "{application} must select only unit tests; keep UI diagnostics in an explicit interface test scheme"
        );
    }
    Ok(())
}

fn build(root: &Path, platform: Platform, release: bool) -> Result<()> {
    generate(root)?;
    let (scheme, destination) = match platform {
        Platform::Macos => ("TungstenMac", "platform=macOS"),
        Platform::Ios => ("TungstenIOS", "generic/platform=iOS Simulator"),
    };
    let mut arguments = vec![
        "-quiet",
        "-project",
        "apple/Tungsten.xcodeproj",
        "-scheme",
        scheme,
        "-destination",
        destination,
        "-configuration",
        if release { "Release" } else { "Debug" },
        "-jobs",
        "2",
        "-derivedDataPath",
        "target/apple-build",
        "build",
        "CODE_SIGN_IDENTITY=-",
    ];
    if matches!(platform, Platform::Ios) {
        arguments.push("CODE_SIGNING_ALLOWED=NO");
    }
    super::run(root, "xcodebuild", &arguments)
}

fn native_path(path: &str) -> bool {
    ["apple/", "src/", "tests/", "xtask/", ".github/workflows/"]
        .iter()
        .any(|prefix| path.starts_with(prefix))
        || [
            "Cargo.toml",
            "Cargo.lock",
            ".cargo/config.toml",
            "ui/package.json",
            "ui/package-lock.json",
            "mise.toml",
            "justfile",
        ]
        .contains(&path)
}

fn affected(root: &Path, base: &str, head: &str, output: &Path) -> Result<()> {
    for revision in [base, head] {
        ensure!(
            revision.len() == 40 && revision.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "the scope check requires exact commit identities"
        );
    }
    let paths = super::output(root, "git", &["diff", "--name-only", base, head, "--"])?;
    let changed = paths.lines().any(native_path);
    writeln!(
        fs::OpenOptions::new().append(true).open(output)?,
        "affected={changed}"
    )?;
    eprintln!("phase=apple-scope affected={changed}");
    Ok(())
}

fn icons(root: &Path) -> Result<()> {
    let developer = PathBuf::from(super::output(root, "xcode-select", &["-p"])?);
    let renderer = developer
        .parent()
        .context("Xcode application contents")?
        .join("Applications/Icon Composer.app/Contents/Executables/ictool");
    let directory = root.join("target/apple-icons");
    fs::create_dir_all(&directory)?;
    for rendition in [
        "Default",
        "Dark",
        "ClearLight",
        "ClearDark",
        "TintedLight",
        "TintedDark",
    ] {
        let image = directory.join(format!("{rendition}.png"));
        super::run(
            root,
            renderer.to_str().context("Icon Composer path")?,
            &[
                "apple/App/Resources/Tungsten.icon",
                "--export-image",
                "--output-file",
                image.to_str().context("icon output path")?,
                "--platform",
                "macOS",
                "--rendition",
                rendition,
                "--width",
                "256",
                "--height",
                "256",
                "--scale",
                "2",
            ],
        )?;
        ensure!(
            fs::metadata(image)?.len() > 0,
            "Icon Composer produced an empty preview"
        );
    }
    Ok(())
}

fn check(root: &Path) -> Result<()> {
    source(root)?;
    let server = TestServer::start(root)?;
    super::run(
        root,
        "xcrun",
        &[
            "swift-format",
            "lint",
            "--strict",
            "--recursive",
            "apple/Sources",
            "apple/Tests",
            "apple/UITests",
            "apple/App",
            "apple/Package.swift",
        ],
    )?;
    super::run_env(
        root,
        "xcrun",
        &[
            "swift",
            "test",
            "--jobs",
            "2",
            "--package-path",
            "apple",
            "--scratch-path",
            "target/apple-package",
            "-Xswiftc",
            "-warnings-as-errors",
        ],
        &[("TUNGSTEN_SERVER", &server.address)],
    )?;
    build(root, Platform::Macos, false)?;
    build(root, Platform::Ios, false)?;
    icons(root)
}

struct TestServer {
    child: Child,
    _directory: tempfile::TempDir,
    address: String,
}

impl TestServer {
    fn start(root: &Path) -> Result<Self> {
        if !root.join("ui/dist/app.js").exists() {
            super::build_ui(root)?;
        }
        super::run(root, "cargo", &["build", "--locked", "--bin", "tungsten"])?;
        let directory = tempfile::tempdir()?;
        let data = directory.path().join("notes");
        {
            let mut store = tungsten::store::Store::open(&data)?;
            let original = store.upload(
                "Native original.txt".to_owned(),
                b"An original file shared with the native apps.\n",
            )?;
            store.create(
                Some("native-linked.md".to_owned()),
                "# Linked Note\n\nA connected note.",
            )?;
            let first = store.create(
                Some("native-start.md".to_owned()),
                "# Native Reference\n\nAn earlier saved version.",
            )?;
            let body = format!(
                "# Native Reference\n\nA shared Markdown note.\n\n[Linked Note](native-linked.md)\n\n[Native original]({})\n",
                original.url
            );
            store.update(&first.name, &body, first.revision.as_str())?;
            let workspace = store.create_workspace("Research")?;
            store.workspace_member(workspace.id, &first.name, true)?;
        }
        let log_path = directory.path().join("server.log");
        let log = fs::File::create(&log_path)?;
        let child = Command::new(root.join("target/debug/tungsten"))
            .args([
                "--data-dir",
                data.to_str().context("native fixture path")?,
                "--listen",
                "127.0.0.1:0",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()?;
        let mut server = Self {
            child,
            _directory: directory,
            address: String::new(),
        };
        let started = Instant::now();
        loop {
            ensure!(
                server.child.try_wait()?.is_none(),
                "native test server exited: {}",
                fs::read_to_string(&log_path)?
            );
            let log = fs::read_to_string(&log_path)?;
            if let Some(address) = log
                .lines()
                .find_map(|line| line.strip_prefix("operation=listen outcome=ready address="))
            {
                let address: SocketAddr = address.parse()?;
                ensure!(
                    address.ip().is_loopback() && address.port() != 0,
                    "unexpected native fixture listener"
                );
                server.address = format!("http://{address}");
                return Ok(server);
            }
            ensure!(
                started.elapsed() < Duration::from_secs(8),
                "native test server did not become ready"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Simulator {
    id: String,
}

impl Simulator {
    fn create(root: &Path) -> Result<Self> {
        let id = super::output(
            root,
            "xcrun",
            &[
                "simctl",
                "create",
                "Tungsten native checks",
                "com.apple.CoreSimulator.SimDeviceType.iPhone-18-Pro",
                "com.apple.CoreSimulator.SimRuntime.iOS-27-2",
            ],
        )?;
        uuid::Uuid::parse_str(&id).context("native simulator identity")?;
        let simulator = Self { id };
        super::run(root, "xcrun", &["simctl", "boot", &simulator.id])?;
        super::run(
            root,
            "xcrun",
            &["simctl", "bootstatus", &simulator.id, "-b"],
        )?;
        Ok(simulator)
    }
}

impl Drop for Simulator {
    fn drop(&mut self) {
        for operation in ["shutdown", "delete"] {
            match Command::new("xcrun")
                .args(["simctl", operation, &self.id])
                .stdout(Stdio::null())
                .status()
            {
                Ok(status) if status.success() => {}
                outcome => {
                    eprintln!("phase=simulator-cleanup operation={operation} outcome={outcome:?}")
                }
            }
        }
    }
}

fn ui_modes(
    platform: Platform,
    tour: bool,
    appearance: Option<Appearance>,
    text_size: Option<TextSize>,
) -> Vec<(&'static str, bool, bool)> {
    if !tour {
        return vec![(
            appearance.unwrap_or(Appearance::Light).name(),
            false,
            text_size.is_some_and(TextSize::enlarged),
        )];
    }
    let modes = match platform {
        Platform::Macos => vec![
            ("Light", false, false),
            ("Dark", false, false),
            ("Light", false, true),
            ("Dark", false, true),
        ],
        Platform::Ios => vec![
            ("Light", false, false),
            ("Dark", false, false),
            ("Light", true, false),
            ("Dark", true, false),
            ("Light", true, true),
            ("Dark", true, true),
        ],
    };
    modes
        .into_iter()
        .filter(|(name, _, large)| {
            appearance.is_none_or(|value| value.name() == *name)
                && text_size.is_none_or(|value| value.enlarged() == *large)
        })
        .collect()
}

fn ui(
    root: &Path,
    platform: Platform,
    tour: bool,
    only: Option<UiScenario>,
    appearance: Option<Appearance>,
    text_size: Option<TextSize>,
) -> Result<()> {
    let lock_path = root.join("target/apple-ui.lock");
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_path)?;
    lock.try_lock()
        .context("another native interface check is using the desktop")?;
    generate(root)?;
    source(root)?;
    let server = TestServer::start(root)?;
    let simulator = match platform {
        Platform::Macos => None,
        Platform::Ios => Some(Simulator::create(root)?),
    };
    let (application, destination) = match simulator.as_ref() {
        Some(simulator) => (
            "TungstenIOS",
            format!("platform=iOS Simulator,id={}", simulator.id),
        ),
        None => ("TungstenMac", "platform=macOS".to_owned()),
    };
    let scheme = format!("{application}InterfaceTests");
    let result_root: PathBuf =
        root.join(format!("target/apple-ui-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir(&result_root)?;
    let mut arguments = vec![
        "-quiet",
        "-project",
        "apple/Tungsten.xcodeproj",
        "-scheme",
        &scheme,
        "-destination",
        &destination,
        "-derivedDataPath",
        "target/apple-ui-build",
        "-parallel-testing-enabled",
        "NO",
        "-jobs",
        "2",
        "CODE_SIGN_IDENTITY=-",
    ];
    arguments.push("build-for-testing");
    super::run(root, "xcodebuild", &arguments)?;
    arguments.pop();
    let modes = ui_modes(platform, tour, appearance, text_size);
    for (index, (appearance, contrast, large)) in modes.into_iter().enumerate() {
        if let Some(simulator) = simulator.as_ref() {
            super::run(
                root,
                "xcrun",
                &[
                    "simctl",
                    "ui",
                    &simulator.id,
                    "appearance",
                    &appearance.to_ascii_lowercase(),
                ],
            )?;
            super::run(
                root,
                "xcrun",
                &[
                    "simctl",
                    "ui",
                    &simulator.id,
                    "content_size",
                    if large {
                        "accessibility-extra-extra-extra-large"
                    } else {
                        "large"
                    },
                ],
            )?;
            super::run(
                root,
                "xcrun",
                &[
                    "simctl",
                    "ui",
                    &simulator.id,
                    "increase_contrast",
                    if contrast { "enabled" } else { "disabled" },
                ],
            )?;
        }
        let result = result_root.join(format!("pass-{index}.xcresult"));
        let mut pass = arguments.clone();
        pass.extend([
            "test-without-building",
            "-resultBundlePath",
            result.to_str().context("native test result path")?,
        ]);
        let selected = only.unwrap_or(UiScenario::Tour).method();
        let filter = format!("-only-testing:{application}UITests/NativeWorkflowTests/{selected}");
        let skip_tour =
            format!("-skip-testing:{application}UITests/NativeWorkflowTests/testNativeScreenTour");
        if only.is_some() || index > 0 {
            pass.push(&filter);
        } else if !tour {
            pass.push(&skip_tour);
        }
        let setting = |enabled| if enabled { "true" } else { "false" };
        super::run_env(
            root,
            "xcodebuild",
            &pass,
            &[
                ("TEST_RUNNER_TUNGSTEN_SERVER", &server.address),
                ("TEST_RUNNER_TUNGSTEN_AUDIT", setting(tour)),
                ("TEST_RUNNER_TUNGSTEN_CONTRAST", setting(contrast)),
                ("TEST_RUNNER_TUNGSTEN_LARGE_TEXT", setting(large)),
                ("TEST_RUNNER_TUNGSTEN_APPEARANCE", appearance),
            ],
        )?;
    }
    if tour && matches!(platform, Platform::Ios) {
        eprintln!("phase=apple-ui text-clipping=manual-review-required");
    }
    eprintln!(
        "phase=apple-ui outcome=passed results={}",
        result_root.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focused_ui_runs_select_only_the_requested_display_conditions() {
        assert_eq!(
            ui_modes(
                Platform::Macos,
                true,
                Some(Appearance::Light),
                Some(TextSize::Enlarged)
            ),
            [("Light", false, true)]
        );
        assert_eq!(
            ui_modes(
                Platform::Ios,
                true,
                Some(Appearance::Dark),
                Some(TextSize::Enlarged)
            ),
            [("Dark", true, true)]
        );
        assert_eq!(
            ui_modes(
                Platform::Macos,
                false,
                Some(Appearance::Dark),
                Some(TextSize::Standard)
            ),
            [("Dark", false, false)]
        );
        assert_eq!(ui_modes(Platform::Macos, true, None, None).len(), 4);
        assert_eq!(ui_modes(Platform::Ios, true, None, None).len(), 6);
    }

    #[test]
    fn standard_test_actions_reject_ui_targets_and_missing_unit_coverage() {
        let valid = serde_json::json!({
            "targets": {
                "Logic": {"type": "bundle.unit-test"},
                "Screen": {"type": "bundle.ui-testing"}
            },
            "schemes": {
                "TungstenMac": {"test": {"targets": ["Logic"]}},
                "TungstenIOS": {"test": {"targets": ["Logic"]}},
                "ExplicitInterfaceTests": {"test": {"targets": ["Screen"]}}
            }
        });
        assert!(check_standard_schemes(&valid).is_ok());
        for application in ["TungstenMac", "TungstenIOS"] {
            for selection in [
                serde_json::json!(["Logic", "Screen"]),
                serde_json::json!(["Unknown"]),
                serde_json::json!([]),
                serde_json::Value::Null,
            ] {
                let mut invalid = valid.clone();
                invalid["schemes"][application]["test"]["targets"] = selection;
                assert!(check_standard_schemes(&invalid).is_err());
            }
        }
    }

    #[test]
    fn interface_checks_require_desktop_access_before_any_effect() {
        let directory = tempfile::tempdir().unwrap();
        for platform in [Platform::Macos, Platform::Ios] {
            for tour in [false, true] {
                let error = run(
                    directory.path(),
                    Action::Ui {
                        platform,
                        tour,
                        only: None,
                        appearance: None,
                        text_size: None,
                        allow_desktop_control: false,
                    },
                )
                .unwrap_err();
                assert!(error.to_string().contains("explicit owner permission"));
                assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
            }
        }
    }

    #[test]
    fn standard_system_components_pass() {
        let source = "TextEditor(text: $body).font(.body).foregroundStyle(Color.primary)";
        assert!(violations(source).is_empty());
        assert!(
            violations("Text(\"Color(red: 1)\") // Color(red: 1)\n/* WKWebView() */").is_empty()
        );
    }

    #[test]
    fn native_scope_includes_service_and_tool_changes() {
        for path in [
            "apple/App/TungstenApp.swift",
            "src/api.rs",
            "xtask/src/apple.rs",
            "Cargo.lock",
            ".cargo/config.toml",
            "ui/package.json",
            "ui/package-lock.json",
            ".github/workflows/ci.yml",
        ] {
            assert!(native_path(path));
        }
        for path in ["README.md", "docs/running.md", "ui/style.css"] {
            assert!(!native_path(path));
        }
    }

    #[test]
    fn custom_design_and_browser_wrappers_fail() {
        for source in [
            "Color(red: 0.1, green: 0.2, blue: 0.3)",
            "Text(\"body\").foregroundStyle(.secondary)",
            "Text(\"body\").font(.system(size: 12))",
            "Text(\"body\").lineLimit(1)",
            "View().minimumScaleFactor(0.5)",
            "View().truncationMode(.tail)",
            "View().toolbarBackground(Color.blue)",
            "View().presentationBackground(Color.white)",
            "View().preferredColorScheme(.light)",
            "View().glassEffect()",
            "WKWebView()",
            "UIWebView()",
        ] {
            assert!(
                !violations(source).is_empty(),
                "accepted prohibited design: {source}"
            );
        }
    }
}
