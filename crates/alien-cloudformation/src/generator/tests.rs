use super::expressions::{is_cloudformation_intrinsic, merge_cf_expression};
use super::{CONDITION_HAS_DOMAIN_NAME, CONDITION_NETWORK_MODE_CREATE};
use crate::template::CfExpression;

#[test]
fn merge_replaces_intrinsic_expression_with_structured_overlay() {
    let mut base = CfExpression::object([(
        "exposure",
        CfExpression::if_(
            CONDITION_HAS_DOMAIN_NAME,
            CfExpression::object([("mode", CfExpression::from("custom"))]),
            CfExpression::object([("mode", CfExpression::from("generated"))]),
        ),
    )]);
    let overlay = CfExpression::object([(
        "exposure",
        CfExpression::object([
            ("mode", CfExpression::from("generated")),
            (
                "certificate",
                CfExpression::object([("mode", CfExpression::from("none"))]),
            ),
        ]),
    )]);

    merge_cf_expression(&mut base, overlay);

    let CfExpression::Object(root) = base else {
        panic!("merged expression should remain an object");
    };
    let exposure = root
        .get("exposure")
        .expect("merged settings should keep exposure");
    let CfExpression::Object(exposure) = exposure else {
        panic!("exposure should be the structured overlay");
    };
    assert_eq!(exposure.get("mode"), Some(&CfExpression::from("generated")));
    assert!(
        !exposure.contains_key("Fn::If"),
        "intrinsic and structured object keys must not be merged"
    );
}

#[test]
fn merge_replaces_structured_expression_with_intrinsic_overlay() {
    let mut base = CfExpression::object([(
        "network",
        CfExpression::object([("type", CfExpression::from("use-default"))]),
    )]);
    let overlay = CfExpression::object([(
        "network",
        CfExpression::if_(
            CONDITION_NETWORK_MODE_CREATE,
            CfExpression::object([("type", CfExpression::from("create"))]),
            CfExpression::no_value(),
        ),
    )]);

    merge_cf_expression(&mut base, overlay);

    let CfExpression::Object(root) = base else {
        panic!("merged expression should remain an object");
    };
    let network = root
        .get("network")
        .expect("merged settings should keep network");
    assert!(
        is_cloudformation_intrinsic(network),
        "intrinsic overlay should replace the structured base"
    );
}
