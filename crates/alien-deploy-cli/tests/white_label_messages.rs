//! The deploy CLI is shipped under the vendor's name. Sentences it prints (progress, results,
//! errors) must not name the product underneath.
//!
//! The scan parses the non-test source of the CLI and its shared helper crate with `syn` and
//! looks at every string literal that is prose (it contains a space), including literals that
//! span several lines and literals inside `format!`/`println!` and other macro calls. Identifiers, paths and environment
//! variable names have no space and are covered by the help-text test in `src/lib.rs` and by
//! the generator tests; this test is about what a person reads.

use std::{fs, path::Path};

use proc_macro2::{TokenStream, TokenTree};
use syn::{visit::Visit, Attribute, LitStr};

/// Prose that may still name the product, each with the reason.
const KNOWN: &[(&str, &str)] = &[
    (
        "Alien Deploy",
        "default display name of the unbranded CLI; a vendor's build replaces it with its own",
    ),
    (
        "alien-deploy sync",
        "follow-up (rename): the sentence hardcodes the unbranded command name; fixing it needs \
         the branded command name passed to the error builders",
    ),
    (
        "alien onboard",
        "follow-up (rename): names the vendor-side command that printed the token",
    ),
    (
        "without alien-deploy builds",
        "follow-up (rename): same hardcoded command name",
    ),
    (
        "alien\\alien-operator.exe",
        "installed file name of the Operator on Windows hosts; existing installs look it up",
    ),
];

/// Environment variable names are an existing, scripted contract: they stay.
fn strip_env_var_names(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("ALIEN_") {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let end = tail
            .find(|c: char| !(c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit()))
            .unwrap_or(tail.len());
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

fn rust_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(dir).expect("read source directory") {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs")
            && !path
                .file_stem()
                .is_some_and(|stem| stem.to_string_lossy().ends_with("_tests"))
        {
            out.push(path);
        }
    }
}

fn is_test_only(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("test")
            || (attr.path().is_ident("cfg")
                && attr
                    .parse_args::<syn::Ident>()
                    .is_ok_and(|ident| ident == "test"))
    })
}

/// Collects every string literal outside test-only items and doc comments.
#[derive(Default)]
struct Literals(Vec<String>);

impl Literals {
    fn tokens(&mut self, stream: TokenStream) {
        for tree in stream {
            match tree {
                TokenTree::Group(group) => self.tokens(group.stream()),
                TokenTree::Literal(literal) => {
                    if let Ok(text) = syn::parse2::<LitStr>(TokenTree::Literal(literal).into()) {
                        self.0.push(text.value());
                    }
                }
                _ => {}
            }
        }
    }
}

impl<'ast> Visit<'ast> for Literals {
    fn visit_lit_str(&mut self, literal: &'ast LitStr) {
        self.0.push(literal.value());
    }
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        self.tokens(mac.tokens.clone());
    }
    fn visit_attribute(&mut self, _: &'ast Attribute) {}
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if !is_test_only(&item.attrs) {
            syn::visit::visit_item_mod(self, item);
        }
    }
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if !is_test_only(&item.attrs) {
            syn::visit::visit_item_fn(self, item);
        }
    }
    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        if !is_test_only(&item.attrs) {
            syn::visit::visit_item_impl(self, item);
        }
    }
    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if !is_test_only(&item.attrs) {
            syn::visit::visit_impl_item_fn(self, item);
        }
    }
    fn visit_item_const(&mut self, item: &'ast syn::ItemConst) {
        if !is_test_only(&item.attrs) {
            syn::visit::visit_item_const(self, item);
        }
    }
    fn visit_item_static(&mut self, item: &'ast syn::ItemStatic) {
        if !is_test_only(&item.attrs) {
            syn::visit::visit_item_static(self, item);
        }
    }
}

/// The prose literals of `source` that name the product and are not in [`KNOWN`].
fn offending_literals(source: &str) -> Vec<String> {
    let file = syn::parse_file(source).expect("parse source");
    let mut literals = Literals::default();
    literals.visit_file(&file);
    literals
        .0
        .into_iter()
        .filter(|literal| {
            literal.contains(' ')
                && strip_env_var_names(literal)
                    .to_ascii_lowercase()
                    .contains("alien")
                && !KNOWN.iter().any(|(text, _)| literal.contains(text))
        })
        .collect()
}

#[test]
fn printed_sentences_never_name_the_product() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    rust_files(&manifest.join("src"), &mut files);
    rust_files(&manifest.join("../alien-cli-common/src"), &mut files);

    let mut offending = Vec::new();
    for file in files {
        let source = fs::read_to_string(&file).expect("read source");
        for literal in offending_literals(&source) {
            offending.push(format!(
                "{}: {}",
                file.strip_prefix(manifest).unwrap_or(&file).display(),
                literal
            ));
        }
    }
    assert!(
        offending.is_empty(),
        "the CLI prints sentences that name the product ({}):\n{}",
        offending.len(),
        offending.join("\n")
    );
}

#[test]
fn a_message_split_over_several_lines_is_caught() {
    // The shape of the old operator hint: the product name sits on a continuation line.
    let source = r#"
        fn hint() -> String {
            format!(
                "Build the operator first with \
                 cargo build -p alien-operator and retry"
            )
        }
        #[cfg(test)]
        mod tests {
            fn ok() { let _ = "alien-operator is named in a test"; }
        }
        /// Doc comments are not output: alien here is ignored.
        fn quiet() {}
    "#;
    let found = offending_literals(source);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("cargo build -p alien-operator"));
}
