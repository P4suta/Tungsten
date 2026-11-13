use anyhow::{Context, Result, bail, ensure};
use std::{fs, path::Path};
use tree_sitter::{Node, Parser};

mod report;
mod runner;
pub(crate) use runner::fingerprint;
pub use runner::{native, verify};

fn text<'a>(node: Node<'_>, source: &'a str) -> Result<&'a str> {
    node.utf8_text(source.as_bytes())
        .context("read TypeScript syntax")
}

fn expression(node: Node<'_>, source: &str, names: &[(&str, &str)]) -> Result<String> {
    match node.kind() {
        "identifier" => names
            .iter()
            .find(|(name, _)| text(node, source).ok() == Some(name))
            .map(|(_, rust)| rust.to_string())
            .context("undeclared variable in proof input"),
        "true" | "false" => Ok(node.kind().to_owned()),
        "parenthesized_expression" => expression(
            node.named_child(0).context("parenthesized expression")?,
            source,
            names,
        ),
        "unary_expression" => {
            ensure!(
                node.child(0).context("unary operator")?.kind() == "!",
                "unsupported unary operation"
            );
            Ok(format!(
                "(!{})",
                expression(node.named_child(0).context("unary operand")?, source, names)?
            ))
        }
        "binary_expression" => {
            let left = node.child_by_field_name("left").context("left operand")?;
            let right = node.child_by_field_name("right").context("right operand")?;
            let operator = text(
                node.child_by_field_name("operator")
                    .context("binary operator")?,
                source,
            )?;
            ensure!(
                matches!(operator, "&&" | "||"),
                "unsupported boolean operator"
            );
            Ok(format!(
                "({} {operator} {})",
                expression(left, source, names)?,
                expression(right, source, names)?
            ))
        }
        other => bail!("unsupported TypeScript expression: {other}"),
    }
}

pub fn translate(source: &str) -> Result<String> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())?;
    let tree = parser
        .parse(source, None)
        .context("parse browser save rules")?;
    ensure!(
        !tree.root_node().has_error(),
        "invalid TypeScript save rules"
    );
    struct BrowserContract {
        source_name: &'static str,
        proof_name: &'static str,
        parameters: &'static [(&'static str, &'static str)],
    }
    let contracts = [
        BrowserContract {
            source_name: "maySave",
            proof_name: "may_save",
            parameters: &[
                ("dirty", "dirty"),
                ("busy", "busy"),
                ("composing", "composing"),
                ("blocked", "blocked"),
            ],
        },
        BrowserContract {
            source_name: "isSaved",
            proof_name: "is_saved",
            parameters: &[
                ("sameText", "same_text"),
                ("hasRevision", "has_revision"),
                ("busy", "busy"),
            ],
        },
        BrowserContract {
            source_name: "mayReadRemote",
            proof_name: "may_read_remote",
            parameters: &[
                ("visible", "visible"),
                ("onNote", "on_note"),
                ("loading", "loading"),
                ("pending", "pending"),
                ("operation", "operation"),
                ("composing", "composing"),
                ("blocked", "blocked"),
            ],
        },
        BrowserContract {
            source_name: "mayApplyRemote",
            proof_name: "may_apply_remote",
            parameters: &[
                ("sameEditor", "same_editor"),
                ("sameGeneration", "same_generation"),
                ("sameRevision", "same_revision"),
                ("loading", "loading"),
                ("pending", "pending"),
                ("operation", "operation"),
                ("composing", "composing"),
                ("blocked", "blocked"),
            ],
        },
        BrowserContract {
            source_name: "hasRemoteConflict",
            proof_name: "has_remote_conflict",
            parameters: &[("dirty", "dirty"), ("sameText", "same_text")],
        },
        BrowserContract {
            source_name: "mayRecoverDraft",
            proof_name: "may_recover_draft",
            parameters: &[
                ("matchesNote", "matches_note"),
                ("legacyOwner", "legacy_owner"),
                ("coordinated", "coordinated"),
                ("activeOwner", "active_owner"),
            ],
        },
    ];
    let mut cursor = tree.root_node().walk();
    let exports = tree
        .root_node()
        .named_children(&mut cursor)
        .collect::<Vec<_>>();
    ensure!(
        exports.len() == contracts.len(),
        "missing or extra browser proof functions"
    );
    let mut result = String::new();
    for (contract, export) in contracts.iter().zip(exports) {
        let expected = contract.source_name;
        let rust_name = contract.proof_name;
        let names = contract.parameters;
        ensure!(
            export.kind() == "export_statement",
            "expected an exported pure function"
        );
        let function = export.named_child(0).context("function declaration")?;
        let mut children = function.walk();
        ensure!(
            !function
                .children(&mut children)
                .any(|child| child.kind() == "async"),
            "async proof input is unsupported"
        );
        ensure!(
            function.kind() == "function_declaration",
            "unsupported export"
        );
        ensure!(
            text(
                function
                    .child_by_field_name("name")
                    .context("function name")?,
                source
            )? == expected,
            "unexpected save rule"
        );
        let parameters = function
            .child_by_field_name("parameters")
            .context("parameters")?;
        let mut cursor = parameters.walk();
        let parameters = parameters.named_children(&mut cursor).collect::<Vec<_>>();
        ensure!(parameters.len() == names.len(), "wrong boolean domain");
        for (parameter, (name, _)) in parameters.iter().zip(names.iter()) {
            ensure!(
                parameter.kind() == "required_parameter",
                "optional or implicit proof input"
            );
            ensure!(
                text(
                    parameter
                        .child_by_field_name("pattern")
                        .context("parameter name")?,
                    source
                )? == *name,
                "wrong parameter meaning"
            );
            ensure!(
                text(
                    parameter
                        .child_by_field_name("type")
                        .context("parameter type")?,
                    source
                )?
                .trim()
                    == ": boolean",
                "nonboolean proof input"
            );
        }
        ensure!(
            text(
                function
                    .child_by_field_name("return_type")
                    .context("return type")?,
                source
            )?
            .trim()
                == ": boolean",
            "nonboolean return"
        );
        let body = function
            .child_by_field_name("body")
            .context("function body")?;
        ensure!(
            body.named_child_count() == 1,
            "side effects or local statements in proof input"
        );
        let returned = body.named_child(0).context("return statement")?;
        ensure!(
            returned.kind() == "return_statement",
            "expected a pure return"
        );
        let value = expression(
            returned.named_child(0).context("returned expression")?,
            source,
            names,
        )?;
        let parameters = names
            .iter()
            .map(|(_, rust)| format!("{rust}: bool"))
            .collect::<Vec<_>>()
            .join(", ");
        let value = value
            .strip_prefix('(')
            .and_then(|value| value.strip_suffix(')'))
            .unwrap_or(&value);
        result.push_str(&format!(
            "pub fn {rust_name}({parameters}) -> bool {{ {value} }}\n"
        ));
    }
    Ok(result)
}

pub fn generate(root: &Path) -> Result<()> {
    let source = fs::read_to_string(root.join("ui/src/model.ts"))?;
    let translated = translate(&source)?;
    fs::create_dir_all(root.join("target"))?;
    fs::write(root.join("target/browser-proof.rs"), translated)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const VALID: &str = include_str!("../../ui/src/model.ts");

    #[test]
    fn translates_production_rules_and_rejects_unmodeled_semantics() {
        assert!(translate(include_str!("../../ui/src/model.ts")).is_ok());
        assert!(
            translate(VALID)
                .unwrap()
                .contains("pub fn may_apply_remote")
        );
        for bad in [
            VALID.replace("return dirty", "console.log(dirty); return dirty"),
            VALID.replace("dirty && !busy", "dirty === !busy"),
            VALID.replace("busy: boolean", "busy: number"),
            VALID.replace("return dirty", "return missing"),
            VALID.replace("export function", "export async function"),
            VALID.replace("blocked: boolean", "blocked?: boolean"),
            VALID.replace("return dirty", "return Boolean(dirty)"),
            VALID.replace("sameEditor: boolean", "otherEditor: boolean"),
            VALID.replace("return sameText", "return (sameText = false)"),
            VALID.replace("return dirty", "return dirty ? true : false"),
            format!("{VALID}\nexport const unchecked = true;"),
            VALID.replace(
                "export function mayRecoverDraft",
                "function mayRecoverDraft",
            ),
        ] {
            assert!(translate(&bad).is_err(), "{bad}");
        }
    }
}
