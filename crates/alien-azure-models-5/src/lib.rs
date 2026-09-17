//! Internal Azure model shard, balanced by generated line count for parallel compilation.

pub mod load_balancer {
    include!(concat!(env!("OUT_DIR"), "/load_balancer.rs"));
}
pub mod containerregistry {
    include!(concat!(env!("OUT_DIR"), "/containerregistry.rs"));
}
pub mod jobs {
    include!(concat!(env!("OUT_DIR"), "/jobs.rs"));
}
pub mod queue_namespace {
    include!(concat!(env!("OUT_DIR"), "/queue_namespace.rs"));
}
pub mod nat_gateway {
    include!(concat!(env!("OUT_DIR"), "/nat_gateway.rs"));
}
