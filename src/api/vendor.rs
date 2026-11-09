use axum::{
    extract::Path,
    http::header,
    response::{IntoResponse, Response},
};
use include_dir::{Dir, include_dir};

static CMAPS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/ui/node_modules/pdfjs-dist/cmaps");
static FONTS: Dir<'_> =
    include_dir!("$CARGO_MANIFEST_DIR/ui/node_modules/pdfjs-dist/standard_fonts");
static DECODERS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/ui/node_modules/pdfjs-dist/wasm");

pub(super) async fn pdf(Path(path): Path<String>) -> Result<Response, crate::Error> {
    let bytes: &'static [u8] = match path.as_str() {
        "LICENSE" => include_bytes!("../../ui/node_modules/pdfjs-dist/LICENSE"),
        "pdf.mjs" => include_bytes!("../../ui/node_modules/pdfjs-dist/build/pdf.mjs"),
        "pdf.worker.mjs" => include_bytes!("../../ui/node_modules/pdfjs-dist/build/pdf.worker.mjs"),
        _ => {
            let (directory, name) = path.split_once('/').ok_or(crate::Error::NotFound)?;
            let directory = match directory {
                "cmaps" => &CMAPS,
                "standard_fonts" => &FONTS,
                "wasm" => &DECODERS,
                _ => return Err(crate::Error::NotFound),
            };
            directory
                .get_file(name)
                .ok_or(crate::Error::NotFound)?
                .contents()
        }
    };
    let mime = if path.ends_with(".mjs") || path.ends_with(".js") {
        "text/javascript"
    } else if path.ends_with(".wasm") {
        "application/wasm"
    } else {
        "application/octet-stream"
    };
    Ok(([(header::CONTENT_TYPE, mime)], bytes).into_response())
}
