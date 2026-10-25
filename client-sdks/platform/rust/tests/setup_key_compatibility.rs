//! Published CloudFormation callback types must survive adding Terraform setup.

#[cfg(feature = "full-api")]
use alien_platform_api::types::AcceptAwsKeySetupBodyAction;

#[cfg(feature = "full-api")]
#[test]
fn cloudformation_key_callback_actions_keep_their_published_type_and_wire_names() {
    for (action, wire) in [
        (AcceptAwsKeySetupBodyAction::Create, "create"),
        (AcceptAwsKeySetupBodyAction::Update, "update"),
        (AcceptAwsKeySetupBodyAction::Delete, "delete"),
    ] {
        assert_eq!(serde_json::to_value(action).unwrap(), wire);
    }
}
