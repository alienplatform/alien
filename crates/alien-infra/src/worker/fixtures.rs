use std::collections::HashMap;

use crate::core::controller_test::{test_storage_1, test_storage_2};
use alien_core::{
    HttpMethod, Queue, ReadinessProbe, ResourceRef, Storage, Worker, WorkerCode,
    WorkerPublicEndpoint, WorkerTrigger,
};
use rstest::fixture;

// Test fixtures for different worker configurations
fn api_endpoint() -> WorkerPublicEndpoint {
    WorkerPublicEndpoint {
        name: "api".to_string(),
        host_label: None,
        wildcard_subdomains: false,
    }
}

#[fixture]
pub(crate) fn basic_function() -> Worker {
    Worker::new("basic-func".to_string())
        .code(WorkerCode::Image {
            image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/basic:latest".to_string(),
        })
        .permissions("default-profile".to_string())
        .build()
}

#[fixture]
pub(crate) fn function_with_env_vars() -> Worker {
    let mut env_vars = HashMap::new();
    env_vars.insert("APP_ENV".to_string(), "production".to_string());
    env_vars.insert("LOG_LEVEL".to_string(), "debug".to_string());
    env_vars.insert("DB_NAME".to_string(), "myapp".to_string());

    Worker::new("env-func".to_string())
        .code(WorkerCode::Image {
            image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/env:latest".to_string(),
        })
        .permissions("default-profile".to_string())
        .environment(env_vars)
        .build()
}

#[fixture]
pub(crate) fn function_with_storage_link() -> Worker {
    Worker::new("storage-func".to_string())
        .code(WorkerCode::Image {
            image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/storage:latest".to_string(),
        })
        .permissions("default-profile".to_string())
        .link(&test_storage_1())
        .build()
}

#[fixture]
pub(crate) fn function_with_env_and_storage() -> Worker {
    let mut env_vars = HashMap::new();
    env_vars.insert("APP_ENV".to_string(), "staging".to_string());
    env_vars.insert("DEBUG".to_string(), "true".to_string());

    Worker::new("env-storage-func".to_string())
        .code(WorkerCode::Image {
            image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/env-storage:latest".to_string(),
        })
        .permissions("default-profile".to_string())
        .environment(env_vars)
        .link(&test_storage_1())
        .link(&test_storage_2())
        .build()
}

#[fixture]
pub(crate) fn function_with_multiple_storages() -> Worker {
    let mut env_vars = HashMap::new();
    env_vars.insert("SERVICE_NAME".to_string(), "multi-storage".to_string());
    env_vars.insert("VERSION".to_string(), "1.2.3".to_string());

    Worker::new("multi-storage-func".to_string())
        .code(WorkerCode::Image {
            image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/multi:latest".to_string(),
        })
        .permissions("default-profile".to_string())
        .environment(env_vars)
        .link(&test_storage_1())
        .link(&test_storage_2())
        .link(&test_storage_1()) // Using storage 1 again for third link since we only have 2 standard storages
        .build()
}

#[fixture]
pub(crate) fn function_public_ingress() -> Worker {
    Worker::new("public-func".to_string())
        .code(WorkerCode::Image {
            image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/public:latest".to_string(),
        })
        .permissions("default-profile".to_string())
        .public_endpoint(api_endpoint())
        .build()
}

#[fixture]
pub(crate) fn function_private_ingress() -> Worker {
    Worker::new("private-func".to_string())
        .code(WorkerCode::Image {
            image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/private:latest".to_string(),
        })
        .permissions("default-profile".to_string())
        .build()
}

#[fixture]
pub(crate) fn function_with_concurrency() -> Worker {
    Worker::new("concurrent-func".to_string())
        .code(WorkerCode::Image {
            image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/concurrent:latest".to_string(),
        })
        .permissions("default-profile".to_string())
        .concurrency_limit(100)
        .build()
}

#[fixture]
pub(crate) fn function_custom_config() -> Worker {
    Worker::new("custom-func".to_string())
        .code(WorkerCode::Image {
            image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/custom:latest".to_string(),
        })
        .permissions("default-profile".to_string())
        .memory_mb(512)
        .timeout_seconds(120)
        .build()
}

#[fixture]
pub(crate) fn function_with_readiness_probe() -> Worker {
    Worker::new("probe-func".to_string())
        .code(WorkerCode::Image {
            image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/probe:latest".to_string(),
        })
        .permissions("default-profile".to_string())
        .public_endpoint(api_endpoint())
        .readiness_probe(ReadinessProbe {
            method: HttpMethod::Get,
            path: "/health/ready".to_string(),
        })
        .build()
}

#[fixture]
pub(crate) fn function_complete_test() -> Worker {
    let mut env_vars = HashMap::new();
    env_vars.insert("ENV_VAR_1".to_string(), "value1".to_string());
    env_vars.insert("ENV_VAR_2".to_string(), "value2".to_string());

    Worker::new("test-worker".to_string())
        .code(WorkerCode::Image {
            image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/my-repo:latest".to_string(),
        })
        .timeout_seconds(30)
        .memory_mb(128)
        .environment(env_vars)
        .permissions("default-profile".to_string())
        .public_endpoint(api_endpoint())
        .readiness_probe(ReadinessProbe {
            path: "/health".to_string(),
            method: HttpMethod::Get,
        })
        .build()
}

#[fixture]
pub(crate) fn test_queue() -> Queue {
    Queue::new("test-queue".to_string()).build()
}

#[fixture]
pub(crate) fn function_with_queue_trigger() -> Worker {
    let queue = test_queue();

    Worker::new("queue-func".to_string())
        .code(WorkerCode::Image {
            image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/queue:latest".to_string(),
        })
        .permissions("default-profile".to_string())
        .trigger(WorkerTrigger::queue(&queue))
        .build()
}
