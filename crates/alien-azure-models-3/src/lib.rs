//! Internal Azure model shard, balanced by generated line count for parallel compilation.

pub mod managed_clusters {
    include!(concat!(env!("OUT_DIR"), "/managed_clusters.rs"));
}
pub mod container_apps {
    include!(concat!(env!("OUT_DIR"), "/container_apps.rs"));
}
pub mod managed_environments {
    include!(concat!(env!("OUT_DIR"), "/managed_environments.rs"));
}
pub mod queue {
    include!(concat!(env!("OUT_DIR"), "/queue.rs"));
}
pub mod authorization_role_definitions {
    include!(concat!(
        env!("OUT_DIR"),
        "/authorization_role_definitions.rs"
    ));
}
