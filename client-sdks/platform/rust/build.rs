use progenitor::{GenerationSettings, InterfaceStyle};

#[path = "build/merged_type_names.rs"]
mod merged_type_names;
#[path = "build/openapi_filter.rs"]
mod openapi_filter;

// The deduplicated filtered client currently generates about 291,000 lines. Leave enough
// room for normal API evolution while catching accidental graph explosions.
const MAX_FILTERED_GENERATED_LINES: usize = 320_000;

fn main() {
    // Spawn on a thread with an 8 MB stack to avoid overflow on Windows (1 MB default).
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(run)
        .unwrap()
        .join()
        .unwrap();
}

fn run() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let src = std::path::Path::new(&manifest_dir).join("openapi.json");
    println!("cargo:rerun-if-changed={}", src.display());
    // Keep the specification in rustc's dependency graph as well as Cargo's
    // build-script watch list so restored build caches cannot reuse code that
    // was generated from an older API schema.
    let full_spec: serde_json::Value = serde_json::from_str(include_str!("openapi.json")).unwrap();
    let content = if cfg!(feature = "full-api") {
        let normalized_spec = openapi_filter::normalize_openapi(&full_spec).unwrap();
        let (merged_spec, unmerged_spec, positioned_spec) =
            openapi_filter::merge_repeated_schemas_with_positions(&normalized_spec).unwrap();
        let mut merged = generate(merged_spec);
        // The other two clients are generated only to learn which type names merging removes
        // and what replaced them, so those names stay available as aliases.
        let unmerged = generate(unmerged_spec);
        let positioned = generate(positioned_spec);
        merged_type_names::add_aliases(&unmerged, &positioned, &mut merged).unwrap();
        prettyplease::unparse(&merged)
    } else {
        let filtered_spec =
            openapi_filter::filter_openapi(&full_spec, openapi_filter::REQUIRED_OPERATION_IDS)
                .unwrap();
        prettyplease::unparse(&generate(filtered_spec))
    };
    if !cfg!(feature = "full-api") {
        let generated_lines = content.lines().count();
        assert!(
            generated_lines <= MAX_FILTERED_GENERATED_LINES,
            "filtered Platform client generated {generated_lines} lines; expected at most {MAX_FILTERED_GENERATED_LINES}"
        );
    }

    let mut out_file = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).to_path_buf();
    out_file.push("codegen.rs");

    std::fs::write(out_file, content).unwrap();
}

fn generate(spec: serde_json::Value) -> syn::File {
    let spec = serde_json::from_value(spec).unwrap();
    let mut generator = progenitor::Generator::new(
        GenerationSettings::new()
            .with_interface(InterfaceStyle::Builder)
            .with_crate("std", progenitor::CrateVers::Any, None),
    );
    syn::parse2(generator.generate_tokens(&spec).unwrap()).unwrap()
}
