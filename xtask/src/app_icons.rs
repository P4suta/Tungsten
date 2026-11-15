use anyhow::{Context, Result, ensure};
use clap::Subcommand;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

const TARGETS: [u32; 14] = [16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 256];
const SCALES: [u32; 5] = [100, 125, 150, 200, 400];
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
const SOURCE_COMMENT: &str = "Tungsten source sha256: ";

#[derive(Subcommand)]
pub enum Action {
    Generate,
    Check,
}

#[derive(Clone, Copy)]
enum Appearance {
    Default,
    Light,
    Dark,
}
impl Appearance {
    fn source(self) -> &'static str {
        match self {
            Self::Default => "assets/app-icon/cube.svg",
            Self::Light => "assets/app-icon/cube-light.svg",
            Self::Dark => "assets/app-icon/cube-dark.svg",
        }
    }
    fn suffix(self) -> &'static str {
        match self {
            Self::Default => "",
            Self::Light => "_altform-lightunplated",
            Self::Dark => "_altform-unplated",
        }
    }
    fn ico(self) -> &'static str {
        match self {
            Self::Default => "windows/Tungsten/Assets/AppIcon.ico",
            Self::Light => "windows/Tungsten/Assets/AppIconLight.ico",
            Self::Dark => "windows/Tungsten/Assets/AppIconDark.ico",
        }
    }
}
const APPEARANCES: [Appearance; 3] = [Appearance::Default, Appearance::Light, Appearance::Dark];

#[derive(PartialEq)]
enum Ink {
    Black,
    White,
}

struct Mark {
    source: String,
    paths: Vec<String>,
    geometry: Vec<String>,
    ink: Vec<Ink>,
    hash: String,
}
fn svg_with_lf(source: &str) -> String {
    source.replace("\r\n", "\n")
}
fn mark(source: String, canvas: u32) -> Result<Mark> {
    ensure!(source.len() <= 64 * 1024, "icon source is too large");
    let source = svg_with_lf(&source);
    let doc = roxmltree::Document::parse(&source)?;
    let root = doc.root_element();
    ensure!(
        root.has_tag_name("svg")
            && root.tag_name().namespace() == Some("http://www.w3.org/2000/svg"),
        "app icon must be SVG"
    );
    let side = canvas.to_string();
    let view_box = format!("0 0 {canvas} {canvas}");
    ensure!(
        root.attribute("viewBox") == Some(view_box.as_str())
            && root.attribute("width") == Some(side.as_str())
            && root.attribute("height") == Some(side.as_str()),
        "app icon must use the {canvas}-pixel canvas"
    );
    for node in doc.descendants().filter(roxmltree::Node::is_element) {
        ensure!(
            node == root || (node.has_tag_name("path") && node.parent() == Some(root)),
            "icon masters must contain filled paths"
        );
        for attribute in node.attributes() {
            ensure!(
                matches!(
                    attribute.name(),
                    "width" | "height" | "viewBox" | "fill" | "fill-rule" | "clip-rule" | "d"
                ),
                "unsupported icon attribute: {}",
                attribute.name()
            );
            if attribute.name() == "fill" {
                ensure!(
                    matches!(attribute.value(), "black" | "white" | "none"),
                    "icon masters must use black and white"
                );
            }
        }
        if node.has_tag_name("path") {
            ensure!(
                matches!(node.attribute("fill"), Some("black" | "white")),
                "icon paths must have a solid fill"
            );
            let data = node.attribute("d").context("missing icon path")?;
            ensure!(
                !data.is_empty()
                    && data.bytes().all(|byte| {
                        b"MmZzLlHhVvCcSsQqTtAa0123456789 +-.eE,\n\r\t".contains(&byte)
                    }),
                "invalid icon path data"
            );
        }
    }
    let paths: Vec<_> = root
        .children()
        .filter(roxmltree::Node::is_element)
        .map(|n| source[n.range()].to_owned())
        .collect();
    let geometry = root
        .children()
        .filter(roxmltree::Node::is_element)
        .map(|n| n.attribute("d").expect("validated icon path").to_owned())
        .collect();
    let ink = root
        .children()
        .filter(roxmltree::Node::is_element)
        .map(|node| match node.attribute("fill") {
            Some("black") => Ink::Black,
            Some("white") => Ink::White,
            _ => unreachable!("validated icon fill"),
        })
        .collect();
    let hash = Sha256::digest(source.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok(Mark {
        source,
        paths,
        geometry,
        ink,
        hash,
    })
}

struct Sources {
    default: Mark,
    light: Mark,
    dark: Mark,
    front: Mark,
}
impl Sources {
    fn load(root: &Path) -> Result<Self> {
        let load = |appearance: Appearance| {
            mark(fs::read_to_string(root.join(appearance.source()))?, 1024)
                .with_context(|| appearance.source())
        };
        let sources = Self {
            default: load(Appearance::Default)?,
            light: load(Appearance::Light)?,
            dark: load(Appearance::Dark)?,
            front: mark(
                fs::read_to_string(root.join("assets/app-icon/cube-front.svg"))?,
                16,
            )?,
        };
        ensure!(
            sources.light.geometry.len() == 1
                && sources.dark.geometry == sources.light.geometry
                && sources.default.geometry.len() == 2
                && sources
                    .default
                    .geometry
                    .contains(&sources.light.geometry[0]),
            "icon appearances must share one filled cube geometry"
        );
        ensure!(
            sources.light.ink == [Ink::Black]
                && sources.dark.ink == [Ink::White]
                && sources.default.ink == [Ink::White, Ink::Black]
                && sources.front.ink == [Ink::Black],
            "icon ink must match its appearance"
        );
        Ok(sources)
    }
    fn get(&self, appearance: Appearance) -> &Mark {
        match appearance {
            Appearance::Default => &self.default,
            Appearance::Light => &self.light,
            Appearance::Dark => &self.dark,
        }
    }
    fn vectors(&self) -> [(String, String); 3] {
        [
            (
                "apple/App/Resources/Tungsten.icon/Assets/Cube.svg".into(),
                self.light.source.clone(),
            ),
            (
                "linux/data/io.github.p4suta.tungsten.svg".into(),
                format!(
                    "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"128\" height=\"128\" viewBox=\"0 0 128 128\">\n<g transform=\"translate(8 13.333333333333334) scale(0.109375)\">\n{}\n</g>\n</svg>\n",
                    self.default.paths.join("\n")
                ),
            ),
            (
                "linux/data/io.github.p4suta.tungsten-symbolic.svg".into(),
                self.front.source.clone(),
            ),
        ]
    }
}

struct Raster {
    path: String,
    appearance: Appearance,
    width: u32,
    height: u32,
    artwork: u32,
    opaque: bool,
}
fn rasters() -> Vec<Raster> {
    let mut plan = Vec::new();
    for appearance in APPEARANCES {
        for size in TARGETS {
            plan.push(Raster {
                path: format!(
                    "windows/Tungsten/Assets/AppIcon.targetsize-{size}{}.png",
                    appearance.suffix()
                ),
                appearance,
                width: size,
                height: size,
                artwork: size,
                opaque: false,
            });
        }
    }
    for scale in SCALES {
        for (name, width, height, artwork) in [
            ("AppIcon", 44, 44, 44),
            ("TileLogo", 150, 150, 150),
            ("PackageLogo", 50, 50, 50),
            ("SplashScreen", 620, 300, 200),
        ] {
            let scaled = |value: u32| (value * scale + 50) / 100;
            plan.push(Raster {
                path: format!("windows/Tungsten/Assets/{name}.scale-{scale}.png"),
                appearance: Appearance::Default,
                width: scaled(width),
                height: scaled(height),
                artwork: scaled(artwork),
                opaque: false,
            });
        }
    }
    plan.push(Raster {
        path: "ui/assets/apple-touch-icon.png".into(),
        appearance: Appearance::Light,
        width: 180,
        height: 180,
        artwork: 180,
        opaque: true,
    });
    plan
}

pub fn run(root: &Path, action: Action) -> Result<()> {
    match action {
        Action::Generate => generate(root),
        Action::Check => check(root),
    }
}

fn generate(root: &Path) -> Result<()> {
    let sources = Sources::load(root)?;
    ensure!(
        super::output(root, "magick", &["-version"])?.contains("ImageMagick 7."),
        "icon rendering requires ImageMagick 7"
    );
    let plan = rasters();
    eprintln!("phase=app-icons operation=generate rasters={}", plan.len());
    for asset in plan {
        let path = root.join(&asset.path);
        fs::create_dir_all(path.parent().context("icon directory")?)?;
        let mut args = vec![
            "-background".to_owned(),
            "none".to_owned(),
            asset.appearance.source().to_owned(),
            "-resize".to_owned(),
            format!("{}x{}", asset.artwork, asset.artwork),
            "-gravity".to_owned(),
            "center".to_owned(),
            "-extent".to_owned(),
            format!("{}x{}", asset.width, asset.height),
        ];
        if asset.opaque {
            args.extend(["-background", "white", "-alpha", "remove"].map(str::to_owned));
        }
        args.extend(["-strip".into(), format!("PNG32:{}", asset.path)]);
        super::output(
            root,
            "magick",
            &args.iter().map(String::as_str).collect::<Vec<_>>(),
        )?;
        fs::write(
            &path,
            stamped_png(
                fs::read(&path)?,
                asset.width,
                asset.height,
                &sources.get(asset.appearance).hash,
            )?,
        )?;
    }
    for (path, svg) in sources.vectors() {
        fs::write(root.join(path), svg)?;
    }
    for appearance in APPEARANCES {
        fs::write(
            root.join(appearance.ico()),
            icon(root, &sources, appearance)?,
        )?;
    }
    fs::copy(
        root.join(Appearance::Default.ico()),
        root.join("ui/assets/favicon.ico"),
    )?;
    for path in [
        "apple/App/Resources/Tungsten.icon/Assets/Note.svg",
        "windows/Tungsten/Assets/LockScreenLogo.scale-200.png",
        "windows/Tungsten/Assets/Square44x44Logo.scale-200.png",
        "windows/Tungsten/Assets/Square150x150Logo.scale-200.png",
        "windows/Tungsten/Assets/Square44x44Logo.targetsize-24_altform-unplated.png",
        "windows/Tungsten/Assets/Square44x44Logo.targetsize-48_altform-lightunplated.png",
        "windows/Tungsten/Assets/StoreLogo.png",
        "windows/Tungsten/Assets/Wide310x150Logo.scale-200.png",
    ] {
        let path = root.join(path);
        if path.exists() {
            fs::remove_file(path)?;
        }
    }
    check(root)
}

pub fn check(root: &Path) -> Result<()> {
    let sources = Sources::load(root)?;
    for asset in rasters() {
        png(
            &fs::read(root.join(&asset.path)).with_context(|| asset.path.clone())?,
            asset.width,
            asset.height,
            &sources.get(asset.appearance).hash,
        )
        .with_context(|| asset.path)?;
    }
    for (path, expected) in sources.vectors() {
        ensure!(
            svg_with_lf(&fs::read_to_string(root.join(&path))?) == expected,
            "regenerate vector icon: {path}"
        );
    }
    for appearance in APPEARANCES {
        ensure!(
            fs::read(root.join(appearance.ico()))? == icon(root, &sources, appearance)?,
            "regenerate ICO: {}",
            appearance.ico()
        );
    }
    ensure!(
        fs::read(root.join("ui/assets/favicon.ico"))?
            == fs::read(root.join(Appearance::Default.ico()))?,
        "Web and Windows must share the default ICO"
    );
    eprintln!("phase=app-icons outcome=passed");
    Ok(())
}

fn icon(root: &Path, sources: &Sources, appearance: Appearance) -> Result<Vec<u8>> {
    let frames = TARGETS
        .into_iter()
        .map(|size| {
            let path = root.join(format!(
                "windows/Tungsten/Assets/AppIcon.targetsize-{size}{}.png",
                appearance.suffix()
            ));
            let bytes = fs::read(path)?;
            png(&bytes, size, size, &sources.get(appearance).hash)?;
            Ok((size, bytes))
        })
        .collect::<Result<Vec<_>>>()?;
    icon_directory(&frames)
}
fn icon_directory(frames: &[(u32, Vec<u8>)]) -> Result<Vec<u8>> {
    ensure!(!frames.is_empty(), "icon must have images");
    let count = u16::try_from(frames.len())?;
    let mut output = vec![0, 0, 1, 0];
    output.extend(count.to_le_bytes());
    let mut offset = u32::from(count) * 16 + 6;
    for (size, data) in frames {
        ensure!(
            (1..=256).contains(size) && !data.is_empty(),
            "invalid icon frame"
        );
        let size = if *size == 256 {
            0
        } else {
            u8::try_from(*size)?
        };
        output.extend([size, size, 0, 0]);
        output.extend(1u16.to_le_bytes());
        output.extend(32u16.to_le_bytes());
        let length = u32::try_from(data.len())?;
        output.extend(length.to_le_bytes());
        output.extend(offset.to_le_bytes());
        offset = offset.checked_add(length).context("icon size overflow")?;
    }
    for (_, data) in frames {
        output.extend(data);
    }
    Ok(output)
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}
fn big_endian(data: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_be_bytes(
        data.get(offset..offset + 4)
            .context("truncated PNG")?
            .try_into()?,
    ))
}

fn stamped_png(mut data: Vec<u8>, width: u32, height: u32, source: &str) -> Result<Vec<u8>> {
    const END: &[u8] = b"\0\0\0\0IEND\xae\x42\x60\x82";
    ensure!(data.ends_with(END), "rendered PNG has no end chunk");
    let text = format!("Comment\0{SOURCE_COMMENT}{source}");
    let mut chunk = b"tEXt".to_vec();
    chunk.extend(text.as_bytes());
    data.truncate(data.len() - END.len());
    data.extend(u32::try_from(text.len())?.to_be_bytes());
    data.extend(&chunk);
    data.extend(crc32(&chunk).to_be_bytes());
    data.extend(END);
    png(&data, width, height, source)?;
    Ok(data)
}

fn png(data: &[u8], width: u32, height: u32, source: &str) -> Result<()> {
    ensure!(
        data.len() <= 4 * 1024 * 1024 && data.starts_with(PNG),
        "invalid PNG signature or size"
    );
    let mut offset = PNG.len();
    let mut header = false;
    let mut pixels = false;
    let mut provenance = false;
    while offset < data.len() {
        let length = usize::try_from(big_endian(data, offset)?)?;
        let end = offset
            .checked_add(length)
            .and_then(|v| v.checked_add(12))
            .context("PNG chunk overflow")?;
        ensure!(end <= data.len(), "truncated PNG chunk");
        let kind = &data[offset + 4..offset + 8];
        let body = &data[offset + 8..end - 4];
        ensure!(
            crc32(&data[offset + 4..end - 4]) == big_endian(data, end - 4)?,
            "invalid PNG checksum"
        );
        match kind {
            b"IHDR" => {
                ensure!(
                    !header && offset == PNG.len() && body.len() == 13,
                    "invalid PNG header"
                );
                ensure!(
                    big_endian(body, 0)? == width
                        && big_endian(body, 4)? == height
                        && body[8..] == [8, 6, 0, 0, 0],
                    "expected an RGBA8 icon at {width} by {height}"
                );
                header = true;
            }
            b"IDAT" => {
                ensure!(header, "PNG image precedes header");
                pixels = true;
            }
            b"tEXt" => {
                if let Some(separator) = body.iter().position(|byte| *byte == 0)
                    && body[..separator].eq_ignore_ascii_case(b"comment")
                {
                    let value = &body[separator + 1..];
                    if let Some(hash) = value.strip_prefix(SOURCE_COMMENT.as_bytes()) {
                        ensure!(
                            !provenance && hash == source.as_bytes(),
                            "stale icon source"
                        );
                        provenance = true;
                    }
                }
            }
            b"IEND" => {
                ensure!(
                    body.is_empty() && end == data.len() && header && pixels && provenance,
                    "incomplete icon PNG"
                );
                return Ok(());
            }
            _ => {}
        }
        offset = end;
    }
    anyhow::bail!("PNG has no end chunk")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exported_assets_survive_windows_checkout_line_endings() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let place = |relative: &str, bytes: &[u8]| {
            let path = directory.path().join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        };
        let sources = Sources::load(root).unwrap();
        let mut vectors = vec!["assets/app-icon/cube-front.svg".to_owned()];
        vectors.extend(APPEARANCES.map(|appearance| appearance.source().to_owned()));
        vectors.extend(sources.vectors().into_iter().map(|(path, _)| path));
        for path in vectors {
            let source = fs::read_to_string(root.join(&path)).unwrap();
            let windows = source.lines().collect::<Vec<_>>().join("\r\n") + "\r\n";
            place(&path, windows.as_bytes());
        }
        for path in rasters()
            .into_iter()
            .map(|asset| asset.path)
            .chain(APPEARANCES.map(|appearance| appearance.ico().to_owned()))
            .chain(std::iter::once("ui/assets/favicon.ico".to_owned()))
        {
            place(&path, &fs::read(root.join(&path)).unwrap());
        }
        check(directory.path()).unwrap();
    }

    #[test]
    fn icon_sources_reject_rounded_strokes_and_external_artwork() {
        let vector = |body: &str| {
            format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1024\" height=\"1024\" viewBox=\"0 0 1024 1024\">{body}</svg>"
            )
        };
        assert!(mark(vector("<path fill=\"black\" d=\"M0 0h8v8H0Z\"/>"), 1024).is_ok());
        for body in [
            "<path stroke=\"black\" stroke-linecap=\"round\" d=\"M0 0L8 8\"/>",
            "<image href=\"https://example.com/logo.png\"/>",
            "<text>T</text>",
            "<path fill=\"#777\" d=\"M0 0h8v8H0Z\"/>",
        ] {
            assert!(mark(vector(body), 1024).is_err(), "accepted {body}");
        }
    }

    #[test]
    fn icon_directories_encode_sizes_offsets_and_rgba_depth() {
        let bytes = icon_directory(&[(16, vec![1, 2]), (256, vec![3, 4, 5])]).unwrap();
        assert_eq!(&bytes[..6], &[0, 0, 1, 0, 2, 0]);
        assert_eq!(&bytes[6..10], &[16, 16, 0, 0]);
        assert_eq!(&bytes[10..14], &[1, 0, 32, 0]);
        assert_eq!(&bytes[18..22], &38u32.to_le_bytes());
        assert_eq!(&bytes[22..26], &[0, 0, 0, 0]);
        assert_eq!(&bytes[34..38], &40u32.to_le_bytes());
        assert_eq!(&bytes[38..], &[1, 2, 3, 4, 5]);
        assert!(icon_directory(&[]).is_err());
        assert!(icon_directory(&[(257, vec![1])]).is_err());
        assert!(icon_directory(&[(16, vec![])]).is_err());
    }

    fn fixture(hash: Option<&str>) -> Vec<u8> {
        let mut result = PNG.to_vec();
        let mut chunk = |kind: &[u8; 4], body: &[u8]| {
            result.extend(u32::try_from(body.len()).unwrap().to_be_bytes());
            result.extend(kind);
            result.extend(body);
            let mut checked = kind.to_vec();
            checked.extend(body);
            result.extend(crc32(&checked).to_be_bytes());
        };
        chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]);
        if let Some(hash) = hash {
            chunk(
                b"tEXt",
                format!("Comment\0{SOURCE_COMMENT}{hash}").as_bytes(),
            );
        }
        chunk(
            b"IDAT",
            &[
                0x78, 0x01, 0x01, 0x05, 0x00, 0xfa, 0xff, 0, 0, 0, 0, 0, 0, 5, 0, 1,
            ],
        );
        chunk(b"IEND", &[]);
        result
    }

    #[test]
    fn png_checks_reject_stale_sources_wrong_sizes_corruption_and_truncation() {
        assert_eq!(crc32(b"123456789"), 0xcbf43926);
        let valid = fixture(Some("source"));
        assert!(png(&valid, 1, 1, "source").is_ok());
        assert!(png(&valid, 2, 1, "source").is_err());
        assert!(png(&valid, 1, 1, "other").is_err());
        for end in [0, 8, 12, valid.len() - 1] {
            assert!(png(&valid[..end], 1, 1, "source").is_err());
        }
        let mut corrupt = valid.clone();
        corrupt[20] ^= 1;
        assert!(png(&corrupt, 1, 1, "source").is_err());
        assert!(png(&fixture(None), 1, 1, "source").is_err());
        let stamped = stamped_png(fixture(None), 1, 1, "source").unwrap();
        assert!(png(&stamped, 1, 1, "source").is_ok());
        assert!(stamped_png(valid, 1, 1, "other").is_err());
    }
}
