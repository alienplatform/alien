pub use alien_infra_local::sandbox::*;

#[cfg(test)]
mod aggregate_tests {
    use super::*;
    use crate::core::{deserialize_controller, serialize_controller, ResourceController};

    #[test]
    fn controller_round_trips_by_tag() {
        let controller = LocalSandboxController::default();
        let value = serialize_controller(&controller).expect("serializes with its tag");
        assert_eq!(value["type"], "LocalSandboxController");

        let restored = deserialize_controller(value).expect("a registered tag must deserialize");
        assert_eq!(restored.controller_type(), controller.controller_type());
    }

    #[test]
    fn the_registry_resolves_a_local_sandbox_controller() {
        let registry = crate::core::ResourceRegistry::with_built_ins();
        let controller = registry
            .get_controller(
                alien_core::Sandbox::RESOURCE_TYPE,
                alien_core::Platform::Local,
            )
            .expect("Local must have a registered Sandbox controller");
        assert_eq!(controller.controller_type(), "LocalSandboxController");
    }
}
