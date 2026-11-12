//! The modifier pass hands a Terraform modifier the module the previous one returned, so a
//! second modifier of one extension sees the `<extension>.tf` the first one wrote.
use alien_extension_sdk::prelude::*;
use indexmap::IndexMap;

#[test]
fn a_second_modifier_of_one_extension_adds_to_the_extension_s_file() {
    let mut files = IndexMap::new();
    files.insert(
        "db.tf".to_string(),
        "resource \"aws_rds_cluster\" \"db\" {\n  engine = \"aurora-postgresql\"\n}\n".to_string(),
    );
    let mut first = TfModule::new("acme", files.clone()).unwrap();
    first.add_resource("aws_route53_zone", "first").unwrap();
    // Between two modifiers the pass puts the changed files in place of the module's.
    files.extend(first.changed_files());

    let mut second = TfModule::new("acme", files).unwrap();
    second.add_resource("aws_route53_zone", "second").unwrap();
    assert_eq!(
        second.changed_files()["acme.tf"],
        "resource \"aws_route53_zone\" \"first\" {\n}\n\nresource \"aws_route53_zone\" \"second\" {\n}\n"
    );
}
