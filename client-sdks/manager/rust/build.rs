use progenitor::{GenerationSettings, InterfaceStyle};

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
    let src = std::path::Path::new(&manifest_dir).join("openapi-3.0.json");
    println!("cargo:rerun-if-changed={}", src.display());
    let file = std::fs::File::open(&src).unwrap();
    let raw_spec: serde_json::Value = serde_json::from_reader(file).unwrap();
    let mut settings = GenerationSettings::new();
    settings.with_interface(InterfaceStyle::Builder);
    // Each binding value also accepts arbitrary JSON expressions. Progenitor
    // flattens mixed primitive/object anyOf into structs, which cannot decode
    // literal values. Preserve those leaf values as JSON; outer bindings stay typed.
    for name in raw_spec["components"]["schemas"].as_object().unwrap().keys() {
        if !name.starts_with("BindingValue_") {
            continue;
        }
        let generated_name = match name.as_str() {
            "BindingValue_String" => "BindingValueString",
            "BindingValue_Option_String" => "BindingValueOptionString",
            "BindingValue_Vec_String" => "BindingValueVecString",
            "BindingValue_u16" => "BindingValueU16",
            "BindingValue_u8" => "BindingValueU8",
            _ => panic!("Unsupported binding value schema: {name}"),
        };
        settings.with_replacement(generated_name, "::serde_json::Value", std::iter::empty());
    }
    let spec = serde_json::from_value(raw_spec).unwrap();
    let mut generator = progenitor::Generator::new(&settings);

    let tokens = generator.generate_tokens(&spec).unwrap();
    let ast = syn::parse2(tokens).unwrap();
    let content = prettyplease::unparse(&ast);

    let mut out_file = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).to_path_buf();
    out_file.push("codegen.rs");

    std::fs::write(out_file, content).unwrap();
}
