//! Internal Azure model shard, balanced by generated line count for parallel compilation.

pub mod compute_rp {
    include!(concat!(env!("OUT_DIR"), "/compute_rp.rs"));
}
pub mod disk_rp {
    include!(concat!(env!("OUT_DIR"), "/disk_rp.rs"));
}
pub mod blob {
    include!(concat!(env!("OUT_DIR"), "/blob.rs"));
}
pub mod managed_environments_dapr_components {
    include!(concat!(
        env!("OUT_DIR"),
        "/managed_environments_dapr_components.rs"
    ));
}
pub mod authorization_role_assignments {
    include!(concat!(
        env!("OUT_DIR"),
        "/authorization_role_assignments.rs"
    ));
}
