use anyhow::{Context, Result, ensure};
use clap::{Subcommand, ValueEnum};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

const RUNTIME_IDENTIFIERS_PROPERTY: &str = "RuntimeIdentifiers=\"win-x64;win-arm64\"";

#[derive(Clone, Copy, ValueEnum)]
pub enum Architecture {
    X64,
    Arm64,
}

#[derive(Subcommand)]
pub enum Action {
    Source,
    Check {
        #[arg(long)]
        native_only: bool,
    },
    Build {
        #[arg(long, value_enum, default_value = "x64")]
        arch: Architecture,
    },
    Run,
    Bundle {
        #[arg(long, value_enum)]
        arch: Architecture,
    },
    Contracts,
    Ui {
        #[arg(long)]
        app: Option<u32>,
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
        Action::Check { native_only } => check(root, native_only),
        Action::Build { arch } => {
            native()?;
            execute(root, "dotnet", build_arguments(arch))
        }
        Action::Run => {
            native()?;
            execute(root, "dotnet", build_arguments(Architecture::X64))?;
            super::run(
                root,
                "winapp",
                &[
                    "run",
                    "windows/Tungsten/Tungsten.csproj",
                    "--no-restore",
                    "-c",
                    "Release",
                    "--arch",
                    "x64",
                    "-p",
                    "Platform=x64",
                    "-p",
                    "RestoreLockedMode=true",
                    "-p",
                    "NuGetLockFilePath=packages.win-x64.lock.json",
                    "--detach",
                    "--json",
                ],
            )
        }
        Action::Bundle { arch } => {
            native()?;
            fs::create_dir_all(root.join("dist/windows"))?;
            execute(root, "dotnet", build_arguments(arch))?;
            execute(root, "winapp", bundle_arguments(arch))?;
            let architecture = match arch {
                Architecture::X64 => "x64",
                Architecture::Arm64 => "arm64",
            };
            let package = root.join(format!("dist/windows/Tungsten-{architecture}.msix"));
            super::run(
                root,
                "dotnet",
                &[
                    "restore",
                    "windows/Tungsten.Tests/Tungsten.Tests.csproj",
                    "--locked-mode",
                ],
            )?;
            super::run_env(
                root,
                "dotnet",
                &[
                    "test",
                    "windows/Tungsten.Tests/Tungsten.Tests.csproj",
                    "--no-restore",
                    "--filter",
                    "Category=Package",
                    "--nologo",
                ],
                &[
                    (
                        "TUNGSTEN_PACKAGE_PATH",
                        package.to_str().context("package path")?,
                    ),
                    ("TUNGSTEN_PACKAGE_ARCH", architecture),
                ],
            )
        }
        Action::Contracts => contracts(root),
        Action::Ui { app } => ui(root, app),
        Action::Affected { base, head, output } => {
            for revision in [&base, &head] {
                ensure!(
                    revision.len() == 40 && revision.bytes().all(|b| b.is_ascii_hexdigit()),
                    "scope requires exact commits"
                );
            }
            let paths = super::output(root, "git", &["diff", "--name-only", &base, &head, "--"])?;
            writeln!(
                fs::OpenOptions::new().append(true).open(output)?,
                "affected={}",
                paths.lines().any(relevant)
            )?;
            Ok(())
        }
    }
}
fn native() -> Result<()> {
    ensure!(cfg!(windows), "WinUI operations require Windows");
    Ok(())
}
fn execute(root: &Path, program: &str, arguments: Vec<String>) -> Result<()> {
    super::run(
        root,
        program,
        &arguments.iter().map(String::as_str).collect::<Vec<_>>(),
    )
}
fn contracts(root: &Path) -> Result<()> {
    if cfg!(unix) {
        super::run(root, "npm", &["--prefix", "ui", "ci"])?;
        super::run(root, "npm", &["--prefix", "ui", "run", "build"])?;
        super::run(root, "cargo", &["build", "--locked", "--bin", "tungsten"])?;
        let metadata: serde_json::Value = serde_json::from_str(&super::output(
            root,
            "cargo",
            &["metadata", "--format-version", "1", "--no-deps"],
        )?)?;
        let executable = PathBuf::from(
            metadata["target_directory"]
                .as_str()
                .context("Cargo target directory")?,
        )
        .join("debug/tungsten");
        super::run(
            root,
            "dotnet",
            &[
                "restore",
                "windows/Tungsten.Tests/Tungsten.Tests.csproj",
                "--locked-mode",
            ],
        )?;
        return super::run_env(
            root,
            "dotnet",
            &[
                "test",
                "windows/Tungsten.Tests/Tungsten.Tests.csproj",
                "--no-restore",
                "--filter",
                "Category=Server",
                "--nologo",
            ],
            &[(
                "TUNGSTEN_SERVER_PATH",
                executable.to_str().context("server executable path")?,
            )],
        );
    }
    let directory = tempfile::tempdir()?;
    let identity = directory.path().join("image-id");
    super::run(
        root,
        "docker",
        &[
            "build",
            "--file",
            "windows/contracts.Dockerfile",
            "--iidfile",
            identity.to_str().context("image identity")?,
            ".",
        ],
    )?;
    let image = fs::read_to_string(identity)?;
    ensure!(
        image.trim().starts_with("sha256:"),
        "missing contract image identity"
    );
    super::run(
        root,
        "docker",
        &[
            "run",
            "--rm",
            "--cpus",
            "2",
            "--memory",
            "4g",
            "--memory-swap",
            "4g",
            "--pids-limit",
            "256",
            image.trim(),
        ],
    )
}
fn check(root: &Path, native_only: bool) -> Result<()> {
    source(root)?;
    super::run(root, "cargo", &["fmt", "--all", "--check"])?;
    super::run(
        root,
        "cargo",
        &[
            "clippy",
            "--package",
            "xtask",
            "--bin",
            "xtask",
            "--locked",
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
            "--package",
            "xtask",
            "--bin",
            "xtask",
            "--locked",
            "--target-dir",
            "target/windows-tests",
        ],
    )?;
    super::run(
        root,
        "dotnet",
        &[
            "restore",
            "windows/Tungsten.Tests/Tungsten.Tests.csproj",
            "--locked-mode",
        ],
    )?;
    for project in [
        "windows/Tungsten.Core/Tungsten.Core.csproj",
        "windows/Tungsten.Tests/Tungsten.Tests.csproj",
    ] {
        super::run(
            root,
            "dotnet",
            &["format", project, "--verify-no-changes", "--no-restore"],
        )?;
    }
    super::run(
        root,
        "dotnet",
        &[
            "test",
            "windows/Tungsten.Tests/Tungsten.Tests.csproj",
            "--no-restore",
            "--filter",
            "Category!=Server&Category!=Package",
            "--nologo",
        ],
    )?;
    if !native_only {
        contracts(root)?;
    }
    if cfg!(windows) {
        for arch in [Architecture::X64, Architecture::Arm64] {
            execute(root, "dotnet", build_arguments(arch))?;
        }
        super::run(
            root,
            "dotnet",
            &[
                "restore",
                "windows/Tungsten/Tungsten.csproj",
                "-p:RuntimeIdentifier=win-x64",
                "-p:Platform=x64",
                &format!("-p:{RUNTIME_IDENTIFIERS_PROPERTY}"),
                "-p:Configuration=Release",
                "-p:NuGetLockFilePath=packages.win-x64.lock.json",
                "--locked-mode",
            ],
        )?;
        super::run_env(
            root,
            "dotnet",
            &[
                "format",
                "windows/Tungsten/Tungsten.csproj",
                "--verify-no-changes",
                "--no-restore",
            ],
            &[
                ("RuntimeIdentifier", "win-x64"),
                ("RuntimeIdentifiers", "win-x64;win-arm64"),
                ("Platform", "x64"),
                ("Configuration", "Release"),
                ("NuGetLockFilePath", "packages.win-x64.lock.json"),
            ],
        )?;
    }
    for project in [
        "windows/Tungsten.Core/Tungsten.Core.csproj",
        "windows/Tungsten.Tests/Tungsten.Tests.csproj",
        "windows/Tungsten/Tungsten.csproj",
    ] {
        let report = super::output(
            root,
            "dotnet",
            &[
                "list",
                project,
                "package",
                "--vulnerable",
                "--include-transitive",
                "--no-restore",
                "--format",
                "json",
            ],
        )?;
        let report: serde_json::Value = serde_json::from_str(&report)?;
        fn vulnerable(value: &serde_json::Value) -> bool {
            match value {
                serde_json::Value::Object(fields) => fields.iter().any(|(key, value)| {
                    key == "vulnerabilities" && value.as_array().is_some_and(|v| !v.is_empty())
                        || vulnerable(value)
                }),
                serde_json::Value::Array(values) => values.iter().any(vulnerable),
                _ => false,
            }
        }
        ensure!(
            !vulnerable(&report),
            "vulnerable NuGet dependencies in {project}"
        );
    }
    if !native_only {
        super::docs(root)?;
    }
    Ok(())
}
fn launch_pid(source: &str) -> Result<u32> {
    let value: serde_json::Value = serde_json::from_str(source)?;
    let pid = u32::try_from(
        value["ProcessId"]
            .as_u64()
            .context("launch returned no ProcessId")?,
    )?;
    ensure!(pid > 0 && value.get("Error").is_none(), "app launch failed");
    Ok(pid)
}
fn ui_controls(value: &serde_json::Value) -> std::collections::BTreeMap<&str, &serde_json::Value> {
    let mut controls = std::collections::BTreeMap::new();
    fn visit<'a>(
        value: &'a serde_json::Value,
        controls: &mut std::collections::BTreeMap<&'a str, &'a serde_json::Value>,
    ) {
        match value {
            serde_json::Value::Object(fields) => {
                if let Some(id) = value["automationId"].as_str() {
                    controls.insert(id, value);
                }
                for child in fields.values() {
                    visit(child, controls);
                }
            }
            serde_json::Value::Array(values) => {
                for child in values {
                    visit(child, controls);
                }
            }
            _ => {}
        }
    }
    visit(value, &mut controls);
    controls
}
fn audit_ui(source: &str) -> Result<()> {
    let value: serde_json::Value = serde_json::from_str(source)?;
    let controls = ui_controls(&value);
    let required = if controls
        .get("ServerAddress")
        .is_some_and(|control| control["isOffscreen"] != true)
    {
        ["ServerAddress", "Connect"]
    } else if controls
        .get("NavigationViewBackButton")
        .is_some_and(|control| control["isOffscreen"] == false && control["isEnabled"] == true)
    {
        ["Editor", "NavigationViewBackButton"]
    } else {
        ["NewNote", "Editor"]
    };
    for id in required {
        ensure!(controls.contains_key(id), "missing native control {id}");
    }
    for id in ["ServerAddress", "Connect", "NewNote", "Editor"] {
        let Some(control) = controls.get(id) else {
            continue;
        };
        ensure!(
            control["name"]
                .as_str()
                .is_some_and(|name| !name.trim().is_empty()),
            "missing accessible name for {id}"
        );
        if id == "ServerAddress" {
            ensure!(control["className"] == "TextBox", "server must use TextBox");
        }
        if id == "Editor" {
            ensure!(
                control["className"] == "RichEditBox",
                "live preview must use RichEditBox"
            );
        }
    }
    if let (Some(toggle), Some(command)) =
        (controls.get("TogglePaneButton"), controls.get("NewNote"))
        && toggle["isOffscreen"] == false
        && command["isOffscreen"] == false
    {
        ensure!(
            !controls_overlap(toggle, command)?,
            "native navigation and New note overlap"
        );
    }
    Ok(())
}
fn open_connection_settings(root: &Path, pid: &str) -> Result<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let value = loop {
        let source = super::output(
            root,
            "winapp",
            &["ui", "inspect", "-a", pid, "--depth", "16", "--json"],
        )?;
        let value: serde_json::Value = serde_json::from_str(&source)?;
        let controls = ui_controls(&value);
        if ["ServerAddress", "ConnectionSettings", "TogglePaneButton"]
            .iter()
            .any(|id| {
                controls
                    .get(id)
                    .is_some_and(|control| control["isOffscreen"] == false)
            })
        {
            break value;
        }
        ensure!(
            std::time::Instant::now() < deadline,
            "native connection navigation did not appear: {value}"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    let controls = ui_controls(&value);
    if controls
        .get("ServerAddress")
        .is_some_and(|control| control["isOffscreen"] == false)
    {
        return Ok(());
    }
    if !controls
        .get("ConnectionSettings")
        .is_some_and(|control| control["isOffscreen"] == false)
    {
        ui_execute(root, pid, &["invoke", "TogglePaneButton"])?;
    }
    ui_execute(root, pid, &["wait-for", "ConnectionSettings", "-t", "5000"])?;
    ui_execute(root, pid, &["invoke", "ConnectionSettings"])
}
fn ui(root: &Path, app: Option<u32>) -> Result<()> {
    native()?;
    let owned = app.is_none();
    let app = match app {
        Some(app) => {
            ensure!(app > 0, "app PID must be positive");
            app
        }
        None => {
            let existing = super::output(
                root,
                "winapp",
                &["ui", "list-windows", "-a", "Tungsten", "--json"],
            )?;
            ensure!(
                serde_json::from_str::<serde_json::Value>(&existing)?
                    .as_array()
                    .is_some_and(Vec::is_empty),
                "close existing Tungsten windows before starting the isolated UI workflow"
            );
            launch_ui(root)?
        }
    };
    let pid = app.to_string();
    let draft_title = format!("Windows UI regression draft {app}");
    let draft_text = format!("# {draft_title}");
    let restored = format!("restored{app}");
    let final_title = format!("Windows UI regression draft **{restored}**");
    let final_heading = format!("# {final_title}");
    let live_text = format!("{final_heading}\n\n**Bold** and *italic* with `code`.\n\nEnd");
    let execute = |args: &[&str]| ui_execute(root, &pid, args);
    let result = (|| -> Result<()> {
        open_connection_settings(root, &pid)?;
        execute(&["wait-for", "ServerAddress", "-t", "5000"])?;
        if owned {
            check_single_instance(root, &pid)?;
        }
        audit_ui(&super::output(
            root,
            "winapp",
            &["ui", "inspect", "-a", &pid, "--depth", "16", "--json"],
        )?)?;
        execute(&["set-value", "ServerAddress", "https://127.0.0.1:1/"])?;
        execute(&["invoke", "Connect"])?;
        execute(&[
            "wait-for",
            "Problem",
            "-p",
            "IsOffscreen",
            "--value",
            "False",
            "-t",
            "5000",
        ])?;
        execute(&["invoke", "NewNote"])?;
        execute(&[
            "wait-for",
            "Editor",
            "-p",
            "IsEnabled",
            "--value",
            "True",
            "-t",
            "3000",
        ])?;
        execute(&["focus", "Editor"])?;
        execute(&[
            "send-keys",
            &draft_text,
            "--verbatim",
            "--via",
            "send-input",
        ])?;
        execute(&["wait-for", "Editor", "--value", &draft_text, "-t", "3000"])?;
        let format: serde_json::Value = serde_json::from_str(&super::output(
            root,
            "winapp",
            &[
                "ui",
                "get-property",
                "Editor",
                "--property",
                "FontWeight",
                "-a",
                &pid,
                "--json",
            ],
        )?)?;
        ensure!(
            format["properties"]["FontWeight"] == "700",
            "live heading is not rendered: {format}"
        );
        let default_size = ui_property(root, &pid, "FontSize")?.parse::<f64>()?;
        execute(&["send-keys", "ctrl+vk=0x6B", "--via", "send-input"])?;
        ensure!(
            ui_property(root, &pid, "FontSize")?.parse::<f64>()? > default_size,
            "zoom shortcut did not enlarge the live editor"
        );
        execute(&["send-keys", "ctrl+0", "--via", "send-input"])?;
        ensure!(
            ui_property(root, &pid, "FontSize")?.parse::<f64>()? == default_size,
            "reset zoom did not restore the default size"
        );
        execute(&["invoke", "CreateWorkspace"])?;
        execute(&["wait-for", "WorkspaceName", "-t", "3000"])?;
        execute(&["send-keys", "esc", "--via", "send-input"])?;
        execute(&["wait-for", "NoteActionDialog", "--gone", "-t", "3000"])?;
        execute(&["focus", "Editor"])?;
        execute(&[
            "send-keys",
            "ctrl+home delete delete",
            "--via",
            "send-input",
        ])?;
        execute(&["wait-for", "Editor", "--value", &draft_title, "-t", "3000"])?;
        let plain: serde_json::Value = serde_json::from_str(&super::output(
            root,
            "winapp",
            &[
                "ui",
                "get-property",
                "Editor",
                "--property",
                "FontWeight",
                "-a",
                &pid,
                "--json",
            ],
        )?)?;
        ensure!(
            plain["properties"]["FontWeight"] == "400",
            "removing Markdown leaves stale heading formatting: {plain}"
        );
        execute(&["send-keys", "# ", "--verbatim", "--via", "send-input"])?;
        execute(&["wait-for", "Editor", "--value", &draft_text, "-t", "3000"])?;
        execute(&["invoke", "NewNote"])?;
        execute(&["invoke", "Drafts"])?;
        let capture = root.join("target").join(format!("windows-ui-{app}.png"));
        execute(&[
            "screenshot",
            "--output",
            capture.to_str().context("UI capture path")?,
        ])?;
        open_draft(root, &pid, &draft_title)?;
        execute(&["wait-for", "Editor", "--value", &draft_text, "-t", "3000"])?;
        execute(&["focus", "Editor"])?;
        execute(&["send-keys", "f6", "--via", "send-input"])?;
        let focused: serde_json::Value = serde_json::from_str(&super::output(
            root,
            "winapp",
            &["ui", "get-focused", "-a", &pid, "--json"],
        )?)?;
        ensure!(
            focused["element"]["automationId"] == "TogglePaneButton",
            "F6 must reach collections"
        );
        execute(&["send-keys", "f6", "--via", "send-input"])?;
        let focused: serde_json::Value = serde_json::from_str(&super::output(
            root,
            "winapp",
            &["ui", "get-focused", "-a", &pid, "--json"],
        )?)?;
        ensure!(
            focused["element"]["className"] == "ListViewItem"
                || focused["element"]["name"] == "Search notes",
            "F6 must reach the note region"
        );
        execute(&["send-keys", "f6", "--via", "send-input"])?;
        let focused: serde_json::Value = serde_json::from_str(&super::output(
            root,
            "winapp",
            &["ui", "get-focused", "-a", &pid, "--json"],
        )?)?;
        ensure!(
            focused["element"]["automationId"] == "Editor",
            "F6 must return from note items to the editor"
        );
        execute(&["send-keys", "ctrl+end", "--via", "send-input"])?;
        execute(&["send-keys", "suffix", "--verbatim", "--via", "send-input"])?;
        let with_undo = format!("{draft_text}suffix");
        execute(&["wait-for", "Editor", "--value", &with_undo, "-t", "3000"])?;
        execute(&["send-keys", "ctrl+z", "--via", "send-input"])?;
        execute(&["wait-for", "Editor", "--value", &draft_text, "-t", "3000"])?;
        execute(&["send-keys", "ctrl+y", "--via", "send-input"])?;
        execute(&["wait-for", "Editor", "--value", &with_undo, "-t", "3000"])?;
        execute(&[
            "send-keys",
            "ctrl+end ctrl+shift+left ctrl+b",
            "--via",
            "send-input",
        ])?;
        let formatted = format!("# Windows UI regression draft **{app}suffix**");
        execute(&["wait-for", "Editor", "--value", &formatted, "-t", "3000"])?;
        execute(&["send-keys", "ctrl+z", "--via", "send-input"])?;
        execute(&["wait-for", "Editor", "--value", &with_undo, "-t", "3000"])?;
        execute(&["send-keys", "ctrl+y", "--via", "send-input"])?;
        execute(&["wait-for", "Editor", "--value", &formatted, "-t", "3000"])?;
        execute(&["send-keys", &restored, "--verbatim", "--via", "send-input"])?;
        execute(&[
            "wait-for",
            "Editor",
            "--value",
            &final_heading,
            "-t",
            "3000",
        ])?;
        execute(&["send-keys", "ctrl+end enter enter", "--via", "send-input"])?;
        execute(&[
            "send-keys",
            "**Bold** and *italic* with `code`.",
            "--verbatim",
            "--via",
            "send-input",
        ])?;
        execute(&["send-keys", "enter enter", "--via", "send-input"])?;
        execute(&["send-keys", "End", "--verbatim", "--via", "send-input"])?;
        expect_ui_source(root, &pid, &live_text)?;
        execute(&["send-keys", "ctrl+a ctrl+x", "--via", "send-input"])?;
        expect_ui_source(root, &pid, "")?;
        execute(&["send-keys", "ctrl+v", "--via", "send-input"])?;
        expect_ui_source(root, &pid, &live_text)?;
        execute(&["send-keys", "pasted", "--verbatim", "--via", "send-input"])?;
        expect_ui_source(root, &pid, &format!("{live_text}pasted"))?;
        execute(&["send-keys", "ctrl+z", "--via", "send-input"])?;
        expect_ui_source(root, &pid, &live_text)?;
        execute(&["send-keys", "ctrl+z", "--via", "send-input"])?;
        expect_ui_source(root, &pid, "")?;
        execute(&["send-keys", "ctrl+y", "--via", "send-input"])?;
        expect_ui_source(root, &pid, &live_text)?;
        execute(&["send-keys", "redone", "--verbatim", "--via", "send-input"])?;
        expect_ui_source(root, &pid, &format!("{live_text}redone"))?;
        execute(&["send-keys", "ctrl+z", "--via", "send-input"])?;
        expect_ui_source(root, &pid, &live_text)?;
        open_connection_settings(root, &pid)?;
        execute(&["set-value", "ServerAddress", "https://cancelled.example/"])?;
        execute(&["invoke", "CancelConnection"])?;
        expect_ui_source(root, &pid, &live_text)?;
        audit_ui(&super::output(
            root,
            "winapp",
            &["ui", "inspect", "-a", &pid, "--depth", "16", "--json"],
        )?)?;
        let capture = root.join("target").join(format!("windows-ui-{app}.png"));
        execute(&[
            "screenshot",
            "--output",
            capture.to_str().context("UI capture path")?,
        ])?;
        ensure!(fs::metadata(capture)?.len() > 0, "UI capture is missing");
        execute(&["focus", "Editor"])?;
        let measurement = || -> Result<(f64, f64)> {
            window_measurement(&super::output(
                root,
                "winapp",
                &[
                    "ui",
                    "inspect",
                    "Editor",
                    "--ancestors",
                    "-a",
                    &pid,
                    "--json",
                ],
            )?)
        };
        let drag = narrow_window_drag(&super::output(
            root,
            "winapp",
            &[
                "ui",
                "inspect",
                "Editor",
                "--ancestors",
                "-a",
                &pid,
                "--json",
            ],
        )?)?;
        execute(&["drag", "--", &drag.from.argument(), &drag.to.argument()])?;
        let (width, scale) = measurement()?;
        ensure!(
            (320.0..=640.0).contains(&(width / scale)),
            "native narrow window is outside the test range: width={width} scale={scale} drag={drag:?}"
        );
        audit_ui(&super::output(
            root,
            "winapp",
            &["ui", "inspect", "-a", &pid, "--depth", "16", "--json"],
        )?)?;
        execute(&[
            "wait-for",
            "NavigationViewBackButton",
            "-p",
            "IsEnabled",
            "--value",
            "True",
            "-t",
            "3000",
        ])?;
        let capture = root
            .join("target")
            .join(format!("windows-ui-narrow-{app}.png"));
        execute(&["send-keys", "ctrl+f", "--via", "send-input"])?;
        execute(&[
            "wait-for",
            "Search",
            "-p",
            "IsOffscreen",
            "--value",
            "False",
            "-t",
            "3000",
        ])?;
        open_draft(root, &pid, &final_title)?;
        execute(&[
            "wait-for",
            "Editor",
            "-p",
            "IsOffscreen",
            "--value",
            "False",
            "-t",
            "3000",
        ])?;
        expect_ui_source(root, &pid, &live_text)?;
        execute(&[
            "screenshot",
            "--output",
            capture.to_str().context("narrow capture path")?,
        ])?;
        execute(&["invoke", "NavigationViewBackButton"])?;
        execute(&[
            "wait-for",
            "Search",
            "-p",
            "IsOffscreen",
            "--value",
            "False",
            "-t",
            "3000",
        ])?;
        Ok(())
    })();
    if owned {
        let _ = execute(&["send-keys", "esc", "--via", "send-input"]);
        let cleanup = execute(&["invoke", "Close"]);
        if let Err(error) = &cleanup {
            eprintln!("phase=ui-close outcome=failed error={error}");
        }
        result?;
        cleanup?;
        let redeployed = launch_ui(root)?.to_string();
        let retained = (|| -> Result<()> {
            open_connection_settings(root, &redeployed)?;
            ui_execute(
                root,
                &redeployed,
                &[
                    "wait-for",
                    "ServerAddress",
                    "--value",
                    "https://127.0.0.1:1/",
                    "-t",
                    "5000",
                ],
            )?;
            ui_execute(root, &redeployed, &["invoke", "CancelConnection"])?;
            ui_execute(root, &redeployed, &["invoke", "Drafts"])?;
            open_draft(root, &redeployed, &final_title)?;
            ui_execute(
                root,
                &redeployed,
                &[
                    "wait-for",
                    "Editor",
                    "--value",
                    &final_heading,
                    "--contains",
                    "-t",
                    "3000",
                ],
            )?;
            expect_ui_source(root, &redeployed, &live_text)
        })();
        let cleanup = ui_execute(root, &redeployed, &["invoke", "Close"]);
        retained?;
        cleanup?;
    } else {
        result?;
    }
    Ok(())
}

fn launch_ui(root: &Path) -> Result<u32> {
    execute(root, "dotnet", build_arguments(Architecture::X64))?;
    launch_pid(&super::output(
        root,
        "winapp",
        &[
            "run",
            "windows/Tungsten/Tungsten.csproj",
            "--no-restore",
            "-c",
            "Release",
            "--arch",
            "x64",
            "-p",
            "Platform=x64",
            "-p",
            "RestoreLockedMode=true",
            "-p",
            "NuGetLockFilePath=packages.win-x64.lock.json",
            "--args",
            "--ui-test",
            "--detach",
            "--json",
        ],
    )?)
}

fn ui_execute(root: &Path, pid: &str, args: &[&str]) -> Result<()> {
    super::run_env(
        root,
        "winapp",
        &ui_arguments(pid, args)?,
        &[("WINAPP_UI_WORKFLOW_ID", &format!("tungsten-ui-{pid}"))],
    )
}

fn ui_arguments<'a>(pid: &'a str, args: &[&'a str]) -> Result<Vec<&'a str>> {
    let (action, arguments) = args.split_first().context("missing native UI action")?;
    let mut command = vec!["ui", *action, "-a", pid, "--json"];
    command.extend_from_slice(arguments);
    Ok(command)
}

fn ui_property(root: &Path, pid: &str, property: &str) -> Result<String> {
    let value: serde_json::Value = serde_json::from_str(&super::output(
        root,
        "winapp",
        &[
            "ui",
            "get-property",
            "Editor",
            "--property",
            property,
            "-a",
            pid,
            "--json",
        ],
    )?)?;
    value["properties"][property]
        .as_str()
        .map(str::to_owned)
        .context("native text property")
}

fn expect_ui_source(root: &Path, pid: &str, expected: &str) -> Result<()> {
    let value: serde_json::Value = serde_json::from_str(&super::output(
        root,
        "winapp",
        &["ui", "get-value", "Editor", "-a", pid, "--json"],
    )?)?;
    let text = value["text"]
        .as_str()
        .context("native Markdown source")?
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    ensure!(
        text.trim_end_matches('\n') == expected,
        "native Markdown source differs: {value}"
    );
    Ok(())
}

fn open_draft(root: &Path, pid: &str, title: &str) -> Result<()> {
    ui_execute(root, pid, &["send-keys", "ctrl+f", "--via", "send-input"])?;
    ui_execute(
        root,
        pid,
        &[
            "wait-for",
            "TextBox",
            "--root",
            "Search",
            "-p",
            "HasKeyboardFocus",
            "--value",
            "True",
            "-t",
            "5000",
        ],
    )?;
    ui_execute(root, pid, &["send-keys", "ctrl+a", "--via", "send-input"])?;
    ui_execute(
        root,
        pid,
        &["send-keys", title, "--verbatim", "--via", "send-input"],
    )?;
    ui_execute(root, pid, &["send-keys", "enter", "--via", "send-input"])?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let row = loop {
        let tree = super::output(
            root,
            "winapp",
            &["ui", "inspect", "-a", pid, "--depth", "16", "--json"],
        )?;
        if let Some(row) = draft_item(&tree, title)? {
            break row;
        }
        ensure!(
            std::time::Instant::now() < deadline,
            "the written draft is missing from the native list: {title}; native tree={tree}"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    ui_execute(root, pid, &["click", &row])
}

fn check_single_instance(root: &Path, primary: &str) -> Result<()> {
    let duplicate = launch_pid(&super::output(
        root,
        "winapp",
        &[
            "run",
            "windows/Tungsten/Tungsten.csproj",
            "-c",
            "Release",
            "--arch",
            "x64",
            "-p",
            "Platform=x64",
            "--no-build",
            "--no-restore",
            "--args",
            "--ui-test",
            "--detach",
            "--json",
        ],
    )?)?
    .to_string();
    if duplicate == primary {
        return Ok(());
    }
    let appeared = super::output(
        root,
        "winapp",
        &[
            "ui",
            "wait-for",
            "ServerAddress",
            "-a",
            &duplicate,
            "-t",
            "3000",
            "--json",
        ],
    );
    if appeared.is_ok() {
        let _ = super::run(
            root,
            "winapp",
            &["ui", "invoke", "Close", "-a", &duplicate, "--json"],
        );
        anyhow::bail!("duplicate activation opened another window instead of redirecting");
    }
    let windows = super::output(
        root,
        "winapp",
        &["ui", "list-windows", "-a", "Tungsten", "--json"],
    )?;
    ensure!(
        serde_json::from_str::<serde_json::Value>(&windows)?
            .as_array()
            .is_some_and(|v| v.len() == 1),
        "duplicate activation must preserve one window"
    );
    Ok(())
}

fn draft_item(source: &str, title: &str) -> Result<Option<String>> {
    fn find(value: &serde_json::Value, title: &str) -> Option<String> {
        match value {
            serde_json::Value::Object(fields) => {
                if value["className"] == "ListViewItem"
                    && value["name"] == title
                    && value["isOffscreen"] == false
                {
                    return value["selector"].as_str().map(str::to_owned);
                }
                fields.values().find_map(|child| find(child, title))
            }
            serde_json::Value::Array(values) => values.iter().find_map(|child| find(child, title)),
            _ => None,
        }
    }
    Ok(find(&serde_json::from_str(source)?, title))
}

fn window_measurement(source: &str) -> Result<(f64, f64)> {
    let value: serde_json::Value = serde_json::from_str(source)?;
    let window = &value["windows"][0];
    let root = &window["elements"][0];
    ensure!(root["type"] == "Window", "missing native window bounds");
    let width = root["width"].as_f64().context("native window width")?;
    let scale = window["scale"].as_f64().context("native window scale")?;
    ensure!(
        width > 0.0 && width < 100_000.0 && scale > 0.0 && scale <= 10.0,
        "invalid native window measurement"
    );
    Ok((width, scale))
}

#[derive(Debug, PartialEq)]
struct ScreenPoint {
    x: f64,
    y: f64,
}

impl ScreenPoint {
    fn argument(&self) -> String {
        format!("{:.0},{:.0}", self.x, self.y)
    }
}

#[derive(Debug, PartialEq)]
struct WindowDrag {
    from: ScreenPoint,
    to: ScreenPoint,
}

fn narrow_window_drag(source: &str) -> Result<WindowDrag> {
    let (width, scale) = window_measurement(source)?;
    let value: serde_json::Value = serde_json::from_str(source)?;
    let root = &value["windows"][0]["elements"][0];
    let coordinate = |name: &str| -> Result<f64> {
        let value = root[name]
            .as_f64()
            .context("missing native window coordinate")?;
        ensure!(
            value.is_finite() && value.abs() < 100_000.0,
            "invalid native window coordinate"
        );
        Ok(value)
    };
    let x = coordinate("x")?;
    let y = coordinate("y")?;
    let height = coordinate("height")?;
    let inset = 2.0 * scale;
    ensure!(
        height > inset * 2.0 && width > 640.0 * scale,
        "native window must begin at a wide size"
    );
    let middle = y + height / 2.0;
    Ok(WindowDrag {
        from: ScreenPoint {
            x: x + width - inset,
            y: middle,
        },
        to: ScreenPoint {
            x: x + 600.0 * scale - inset,
            y: middle,
        },
    })
}

fn controls_overlap(first: &serde_json::Value, second: &serde_json::Value) -> Result<bool> {
    let rectangle = |value: &serde_json::Value| -> Result<[f64; 4]> {
        ["x", "y", "width", "height"]
            .map(|key| {
                value[key]
                    .as_f64()
                    .with_context(|| format!("control {key}"))
            })
            .into_iter()
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .map_err(|_| anyhow::anyhow!("control rectangle"))
    };
    let [x, y, width, height] = rectangle(first)?;
    let [other_x, other_y, other_width, other_height] = rectangle(second)?;
    Ok(width > 0.0
        && height > 0.0
        && other_width > 0.0
        && other_height > 0.0
        && x < other_x + other_width
        && x + width > other_x
        && y < other_y + other_height
        && y + height > other_y)
}
pub fn source(root: &Path) -> Result<()> {
    super::app_icons::check(root)?;
    fn visit(path: &Path) -> Result<()> {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                if !["bin", "obj"].contains(&entry.file_name().to_string_lossy().as_ref()) {
                    visit(&path)?;
                }
            } else if path.extension().is_some_and(|e| e == "xaml") {
                xaml(&fs::read_to_string(&path)?).with_context(|| path.display().to_string())?;
            } else if path.extension().is_some_and(|e| e == "cs") {
                csharp(&fs::read_to_string(&path)?).with_context(|| path.display().to_string())?;
            }
        }
        Ok(())
    }
    visit(&root.join("windows"))
}
fn xaml(source: &str) -> Result<()> {
    let document = roxmltree::Document::parse(source)?;
    for node in document.descendants().filter(|n| n.is_element()) {
        let name = node.tag_name().name();
        ensure!(
            ![
                "WebView",
                "WebView2",
                "WebBrowser",
                "SolidColorBrush",
                "LinearGradientBrush",
                "RadialGradientBrush"
            ]
            .contains(&name),
            "use native controls and system resources: {name}"
        );
        for attribute in node.attributes() {
            let (name, value) = (attribute.name(), attribute.value());
            ensure!(
                ![
                    "FontFamily",
                    "FontSize",
                    "Opacity",
                    "HighContrastAdjustment",
                    "ControlTemplate"
                ]
                .contains(&name),
                "use system typography and contrast: {name}"
            );
            if ["Background", "Foreground", "BorderBrush", "Fill", "Stroke"].contains(&name) {
                ensure!(
                    value.starts_with("{ThemeResource "),
                    "use ThemeResource for {name}"
                );
            }
            ensure!(
                name != "IsTextScaleFactorEnabled" || value != "False",
                "system text scaling must remain enabled"
            );
            ensure!(
                name != "RequestedTheme" || value == "Default",
                "follow the system theme"
            );
        }
    }
    Ok(())
}
fn csharp(source: &str) -> Result<()> {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&tree_sitter_c_sharp::LANGUAGE.into())?;
    let tree = parser.parse(source, None).context("C# syntax tree")?;
    ensure!(!tree.root_node().has_error(), "invalid C# syntax");
    let mut pending = vec![tree.root_node()];
    while let Some(node) = pending.pop() {
        let text = node.utf8_text(source.as_bytes())?;
        if node.kind() == "using_directive" {
            ensure!(
                text.split_whitespace().collect::<String>() != "usingWindows.Storage.Pickers;",
                "use Windows App SDK pickers for WinUI"
            );
        }
        if node.kind() == "identifier" {
            ensure!(
                text != "AccessibilitySettings",
                "use Windows App SDK ThemeSettings for desktop contrast"
            );
            ensure!(
                ![
                    "WebView",
                    "WebView2",
                    "WebBrowser",
                    "SolidColorBrush",
                    "Colors",
                    "ColorHelper",
                    "FontFamily",
                    "Opacity"
                ]
                .contains(&text),
                "use native system styling: {text}"
            );
        }
        if node.kind() == "assignment_expression" {
            let left = node
                .child_by_field_name("left")
                .context("assignment target")?
                .utf8_text(source.as_bytes())?;
            if left.ends_with(".FontSize") {
                let right = node
                    .child_by_field_name("right")
                    .context("font scale")?
                    .utf8_text(source.as_bytes())?;
                ensure!(
                    right.split_whitespace().collect::<String>() == "defaultFontSize*zoom",
                    "font size must scale the system default"
                );
            }
        }
        let mut cursor = node.walk();
        pending.extend(node.named_children(&mut cursor));
    }
    Ok(())
}
fn relevant(path: &str) -> bool {
    [
        "windows/",
        "assets/",
        "src/",
        "tests/",
        "xtask/",
        ".github/workflows/",
    ]
    .iter()
    .any(|p| path.starts_with(p))
        || [
            "Cargo.toml",
            "Cargo.lock",
            "mise.toml",
            "justfile",
            ".cargo/config.toml",
        ]
        .contains(&path)
}
fn build_arguments(arch: Architecture) -> Vec<String> {
    let (platform, runtime) = match arch {
        Architecture::X64 => ("x64", "win-x64"),
        Architecture::Arm64 => ("ARM64", "win-arm64"),
    };
    [
        "build",
        "windows/Tungsten/Tungsten.csproj",
        "--configuration",
        "Release",
        "--runtime",
        runtime,
        &format!("-p:Platform={platform}"),
        &format!("-p:{RUNTIME_IDENTIFIERS_PROPERTY}"),
        &format!("-p:NuGetLockFilePath=packages.{runtime}.lock.json"),
        "-p:RestoreLockedMode=true",
        "--nologo",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}
fn bundle_arguments(arch: Architecture) -> Vec<String> {
    let (architecture, platform) = match arch {
        Architecture::X64 => ("x64", "x64"),
        Architecture::Arm64 => ("arm64", "ARM64"),
    };
    [
        "package",
        "windows/Tungsten/Tungsten.csproj",
        "--arch",
        architecture,
        "--no-sign",
        "--self-contained",
        "--no-restore",
        "-p",
        "RestoreLockedMode=true",
        "-p",
        &format!("Platform={platform}"),
        "-p",
        &format!("NuGetLockFilePath=packages.win-{architecture}.lock.json"),
        "--output",
        &format!("dist/windows/Tungsten-{architecture}.msix"),
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_audit_rejects_the_narrow_navigation_collision() {
        let mut tree = serde_json::json!({"elements":[
            {"automationId":"ServerAddress","name":"Server","className":"TextBox"},
            {"automationId":"Connect","name":"Connect"},
            {"automationId":"Editor","name":"Editor","className":"RichEditBox"},
            {"automationId":"TogglePaneButton","isOffscreen":false,"x":381,"y":471,"width":60,"height":54},
            {"automationId":"NewNote","name":"New note","isOffscreen":false,"x":382,"y":464,"width":151,"height":71}
        ]});
        assert!(audit_ui(&tree.to_string()).is_err());
        tree["elements"][4]["y"] = 600.into();
        assert!(audit_ui(&tree.to_string()).is_ok());
    }

    #[test]
    fn native_layout_measurements_reject_missing_bounds_and_overlapping_controls() {
        assert_eq!(
            window_measurement(
                r#"{"windows":[{"scale":1.5,"elements":[{"type":"Window","width":1920}]}]}"#
            )
            .unwrap(),
            (1920.0, 1.5)
        );
        assert!(window_measurement(r#"{"windows":[]}"#).is_err());
        assert!(
            window_measurement(
                r#"{"windows":[{"scale":0,"elements":[{"type":"Window","width":1920}]}]}"#
            )
            .is_err()
        );
        let toggle = serde_json::json!({"x":381,"y":471,"width":60,"height":54});
        let command = serde_json::json!({"x":382,"y":464,"width":151,"height":71});
        assert!(controls_overlap(&toggle, &command).unwrap());
        assert!(
            !controls_overlap(
                &toggle,
                &serde_json::json!({"x":441,"y":471,"width":151,"height":71})
            )
            .unwrap()
        );
    }

    #[test]
    fn native_resize_uses_screen_bounds_and_effective_pixels() {
        for (scale, x, y, width, height, from, to) in [
            (1.0, 100, 200, 754, 512, "852,456", "698,456"),
            (1.5, -1920, -200, 1800, 1050, "-123,325", "-1023,325"),
        ] {
            let source = serde_json::json!({"windows":[{"scale":scale,"elements":[{
                "type":"Window","x":x,"y":y,"width":width,"height":height
            }]}]});
            let drag = narrow_window_drag(&source.to_string()).unwrap();
            assert_eq!(drag.from.argument(), from);
            assert_eq!(drag.to.argument(), to);
        }
    }

    #[test]
    fn native_ui_options_precede_the_negative_coordinate_separator() {
        assert_eq!(
            ui_arguments("42", &["drag", "--", "-123,325", "-1023,325"]).unwrap(),
            [
                "ui",
                "drag",
                "-a",
                "42",
                "--json",
                "--",
                "-123,325",
                "-1023,325"
            ]
        );
        assert_eq!(
            ui_arguments("42", &["send-keys", "ctrl+y", "--via", "send-input"]).unwrap(),
            [
                "ui",
                "send-keys",
                "-a",
                "42",
                "--json",
                "ctrl+y",
                "--via",
                "send-input"
            ]
        );
        assert!(ui_arguments("42", &[]).is_err());
    }

    #[test]
    fn native_resize_rejects_incomplete_or_unusable_window_bounds() {
        let source = serde_json::json!({"windows":[{"scale":1.0,"elements":[{
            "type":"Window","x":100,"y":200,"width":754,"height":512
        }]}]});
        for name in ["x", "y", "width", "height"] {
            for invalid in [serde_json::Value::Null, "100".into(), 100_000.into()] {
                let mut invalid_source = source.clone();
                invalid_source["windows"][0]["elements"][0][name] = invalid;
                assert!(narrow_window_drag(&invalid_source.to_string()).is_err());
            }
        }
        for (name, value) in [("height", 0), ("height", -512), ("width", 640)] {
            let mut invalid_source = source.clone();
            invalid_source["windows"][0]["elements"][0][name] = value.into();
            assert!(narrow_window_drag(&invalid_source.to_string()).is_err());
        }
        for scale in [0.0, -1.0, 11.0] {
            let mut invalid_source = source.clone();
            invalid_source["windows"][0]["scale"] = scale.into();
            assert!(narrow_window_drag(&invalid_source.to_string()).is_err());
        }
        assert!(narrow_window_drag("{}").is_err());
        assert!(narrow_window_drag("{").is_err());
    }

    #[test]
    fn draft_recovery_requires_a_list_item_instead_of_the_editor_title() {
        let source = r#"{"elements":[{"name":"Recovered writing","className":"TextBlock","selector":"heading","isOffscreen":false},{"name":"Recovered writing","className":"ListViewItem","selector":"draft-row","isOffscreen":false}]}"#;
        assert_eq!(
            draft_item(source, "Recovered writing").unwrap().as_deref(),
            Some("draft-row")
        );
        assert!(
            draft_item(
                r#"{"name":"Recovered writing","className":"TextBlock","selector":"heading"}"#,
                "Recovered writing"
            )
            .unwrap()
            .is_none()
        );
        assert!(draft_item(source, "Other writing").unwrap().is_none());
        assert!(
            draft_item(
                &source.replace("\"isOffscreen\":false", "\"isOffscreen\":true"),
                "Recovered writing"
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn standard_controls_and_system_resources_are_admitted() {
        assert!(xaml(r#"<Grid><TextBox Background="{ThemeResource ControlFillColorDefaultBrush}" AcceptsReturn="True"/></Grid>"#).is_ok());
        assert!(
            csharp("class Screen { void Zoom() { editor.FontSize = defaultFontSize * zoom; } }")
                .is_ok()
        );
    }
    #[test]
    fn browser_surfaces_fixed_colors_fonts_and_disabled_scaling_are_refused() {
        for source in [
            "<Grid><WebView2/></Grid>",
            "<TextBox Background=\"Black\"/>",
            "<TextBox Foreground=\"#ffffff\"/>",
            "<TextBox FontSize=\"14\"/>",
            "<TextBox FontFamily=\"Arial\"/>",
            "<TextBox IsTextScaleFactorEnabled=\"False\"/>",
            "<Grid><SolidColorBrush Color=\"Red\"/></Grid>",
            "<Grid Opacity=\"0.5\"/>",
        ] {
            assert!(xaml(source).is_err(), "admitted {source}");
        }
        for source in [
            "class Screen { WebView2 browser; }",
            "class Screen { void Set() { var brush = new SolidColorBrush(); } }",
            "class Screen { void Set() { var color = Colors.Red; } }",
            "class Screen { void Set() { editor.FontSize = 14; } }",
            "using Custom = Microsoft.UI.Xaml.Controls.WebView2; class Screen { Custom browser; }",
            "using Windows.Storage.Pickers; class Screen { FileOpenPicker picker; }",
        ] {
            assert!(csharp(source).is_err(), "admitted {source}");
        }
    }
    #[test]
    fn malformed_sources_never_receive_a_clean_result() {
        assert!(xaml("<Grid>").is_err());
        assert!(csharp("class {").is_err());
    }
    #[test]
    fn desktop_contrast_uses_window_bound_theme_settings() {
        assert!(csharp("using Microsoft.UI.System; class Screen { ThemeSettings settings; void Bind(Microsoft.UI.WindowId id) { settings = ThemeSettings.CreateForWindowId(id); } }").is_ok());
        for source in [
            "using Windows.UI.ViewManagement; class Screen { AccessibilitySettings settings = new(); }",
            "using Contrast = Windows.UI.ViewManagement.AccessibilitySettings; class Screen { Contrast settings = new(); }",
        ] {
            assert!(csharp(source).is_err(), "admitted {source}");
        }
    }
    #[test]
    fn windows_and_shared_tooling_changes_select_the_windows_job() {
        for path in [
            "windows/Tungsten/MainPage.xaml",
            "assets/app-icon/cube.svg",
            "src/api.rs",
            "tests/api.rs",
            "xtask/src/main.rs",
            "mise.toml",
            "Cargo.lock",
            "justfile",
            ".github/workflows/ci.yml",
        ] {
            assert!(relevant(path), "missed {path}");
        }
        assert!(!relevant("docs/features.md"));
        assert!(!relevant("ui/src/app.ts"));
    }
    #[test]
    fn architecture_commands_use_locked_dependencies_and_unsigned_candidates() {
        for (arch, platform, runtime) in [
            (Architecture::X64, "x64", "win-x64"),
            (Architecture::Arm64, "ARM64", "win-arm64"),
        ] {
            let build = build_arguments(arch);
            assert!(build.contains(&format!("-p:{RUNTIME_IDENTIFIERS_PROPERTY}")));
            assert!(build.contains(&format!("-p:Platform={platform}")));
            assert!(build.contains(&runtime.to_owned()));
            assert!(build.contains(&"-p:RestoreLockedMode=true".to_owned()));
            assert!(build.contains(&format!(
                "-p:NuGetLockFilePath=packages.{runtime}.lock.json"
            )));
            let bundle = bundle_arguments(arch);
            assert!(bundle.contains(&"--no-sign".to_owned()));
            assert!(bundle.contains(&"--self-contained".to_owned()));
            assert!(bundle.contains(&"--no-restore".to_owned()));
            assert!(bundle.contains(&format!("Platform={platform}")));
            assert!(bundle.contains(&format!("NuGetLockFilePath=packages.{runtime}.lock.json")));
            assert!(!bundle.contains(&"--cert".to_owned()));
            assert!(bundle.contains(&format!(
                "dist/windows/Tungsten-{}.msix",
                runtime.trim_start_matches("win-")
            )));
        }
    }
    #[test]
    fn ui_workflows_require_a_real_process_and_nonempty_named_native_controls() {
        assert_eq!(launch_pid(r#"{"ProcessId":42}"#).unwrap(), 42);
        for source in ["{}", r#"{"ProcessId":0}"#, r#"{"Error":"Build failed"}"#] {
            assert!(launch_pid(source).is_err());
        }
        for source in [
            "{}",
            r#"{"windows":[]}"#,
            r#"{"windows":[{"elements":[{"automationId":"ServerAddress","name":""}]}]}"#,
        ] {
            assert!(audit_ui(source).is_err());
        }
        let valid = r#"{"windows":[{"elements":[{"automationId":"ServerAddress","name":"Server address","className":"TextBox"},{"automationId":"Connect","name":"Connect"},{"automationId":"NewNote","name":"New note"},{"automationId":"Editor","name":"Live Preview Markdown editor","className":"RichEditBox"}]}]}"#;
        assert!(audit_ui(valid).is_ok());
        assert!(audit_ui(&valid.replace("RichEditBox", "TextBox")).is_err());
        let connection = r#"{"elements":[{"automationId":"ServerAddress","name":"Server address","className":"TextBox","isOffscreen":false},{"automationId":"Connect","name":"Connect"}]}"#;
        assert!(audit_ui(connection).is_ok());
        assert!(audit_ui(&connection.replace("TextBox", "TextBlock")).is_err());
        let writing = r#"{"elements":[{"automationId":"NewNote","name":"New note"},{"automationId":"Editor","name":"Markdown editor","className":"RichEditBox"}]}"#;
        assert!(audit_ui(writing).is_ok());
        assert!(audit_ui(&writing.replace("NewNote", "Missing")).is_err());
        let stacked = r#"{"elements":[{"automationId":"NavigationViewBackButton","name":"Back","isOffscreen":false,"isEnabled":true},{"automationId":"Editor","name":"Markdown editor","className":"RichEditBox"}]}"#;
        assert!(audit_ui(stacked).is_ok());
        assert!(audit_ui(&stacked.replace("RichEditBox", "TextBox")).is_err());
        assert!(audit_ui(&stacked.replace("\"isEnabled\":true", "\"isEnabled\":false")).is_err());
    }
}
