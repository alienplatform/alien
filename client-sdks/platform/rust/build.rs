use progenitor::{GenerationSettings, InterfaceStyle};

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let src = std::path::Path::new(&manifest_dir).join("openapi.json");
    println!("cargo:rerun-if-changed={}", src.display());
    let file = std::fs::File::open(&src).unwrap();
    let spec = serde_json::from_reader(file).unwrap();
    let mut generator = progenitor::Generator::new(
        GenerationSettings::new().with_interface(InterfaceStyle::Builder),
    );

    let tokens = generator.generate_tokens(&spec).unwrap();
    let ast = syn::parse2(tokens).unwrap();
    let content = prettyplease::unparse(&ast);

    let mut out_file = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).to_path_buf();
    out_file.push("codegen.rs");

    std::fs::write(out_file, content).unwrap();
}
