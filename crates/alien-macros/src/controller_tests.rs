use super::HandlerAttr;

#[test]
fn accepts_optional_static_progress() {
    let plain: HandlerAttr = syn::parse_quote! {
        state = Waiting, on_failure = Failed, status = ResourceStatus::Provisioning,
    };
    assert!(plain.progress.is_none());
    let annotated: HandlerAttr = syn::parse_quote! {
        state = Waiting, on_failure = Failed, status = ResourceStatus::Provisioning,
        progress = "Waiting for readiness.",
    };
    assert_eq!(
        annotated.progress.unwrap().value(),
        "Waiting for readiness."
    );
}

#[test]
fn rejects_unknown_and_nonliteral_progress_attributes() {
    for input in [
        "state = Waiting, on_failure = Failed, status = ResourceStatus::Provisioning, progres = ready",
        "state = Waiting, on_failure = Failed, status = ResourceStatus::Provisioning, progress = message",
        "state = Waiting, on_failure = Failed, status = ResourceStatus::Provisioning, progress = 42",
        "state = Waiting, on_failure = Failed, status = ResourceStatus::Provisioning, progress = \"one\", progress = \"two\"",
    ] {
        assert!(syn::parse_str::<HandlerAttr>(input).is_err(), "{input}");
    }
}
