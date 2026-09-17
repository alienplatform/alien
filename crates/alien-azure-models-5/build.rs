fn main() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    alien_azure_model_generator::generate_azure_models(
        &alien_azure_model_generator::model_specs(5),
        &manifest.join("openapi"),
        alien_azure_model_generator::MODEL_ROOTS_JSON,
        cfg!(feature = "full-models"),
        34_000,
    );
}
