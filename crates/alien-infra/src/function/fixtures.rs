use std::collections::HashMap;

use crate::core::controller_test::{test_storage_1, test_storage_2};
use alien_core::{
    Function, FunctionCode, FunctionTrigger, HttpMethod, Ingress, Queue, ReadinessProbe,
    ResourceRef, Storage,
};
use rstest::fixture;

// Test fixtures for different function configurations
#[fixture]
pub(crate) fn basic_function() -> Function {
    Function::new("basic-func".to_string())
        .code(FunctionCode::Image {
            image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/basic:latest".to_string(),
        })
        .permissions("default-profile".to_string())
        .build()
}

#[fixture]
pub(crate) fn function_with_env_vars() -> Function {
    let mut env_vars = HashMap::new();
    env_vars.insert("APP_ENV".to_string(), "production".to_string());
    env_vars.insert("LOG_LEVEL".to_string(), "debug".to_string());
    env_vars.insert("DB_NAME".to_string(), "myapp".to_string());

    Function::new("env-func".to_string())
        .code(FunctionCode::Image {
            image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/env:latest".to_string(),
        })
        .permissions("default-profile".to_string())
        .environment(env_vars)
        .build()
}

#[fixture]
pub(crate) fn function_with_storage_link() -> Function {
    Function::new("storage-func".to_string())
        .code(FunctionCode::Image {
            image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/storage:latest".to_string(),
        })
        .permissions("default-profile".to_string())
        .link(&test_storage_1())
        .build()
}

#[fixture]
pub(crate) fn function_with_env_and_storage() -> Function {
    let mut env_vars = HashMap::new();
    env_vars.insert("APP_ENV".to_string(), "staging".to_string());
    env_vars.insert("DEBUG".to_string(), "true".to_string());

    Function::new("env-storage-func".to_string())
        .code(FunctionCode::Image {
            image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/env-storage:latest".to_string(),
        })
        .permissions("default-profile".to_string())
        .environment(env_vars)
        .link(&test_storage_1())
        .link(&test_storage_2())
        .build()
}

#[fixture]
pub(crate) fn function_with_multiple_storages() -> Function {
    let mut env_vars = HashMap::new();
    env_vars.insert("SERVICE_NAME".to_string(), "multi-storage".to_string());
    env_vars.insert("VERSION".to_string(), "1.2.3".to_string());

    Function::new("multi-storage-func".to_string())
        .code(FunctionCode::Image {
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
pub(crate) fn function_public_ingress() -> Function {
    Function::new("public-func".to_string())
        .code(FunctionCode::Image {
            image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/public:latest".to_string(),
        })
        .permissions("default-profile".to_string())
        .ingress(Ingress::Public)
        .build()
}

#[fixture]
pub(crate) fn function_private_ingress() -> Function {
    Function::new("private-func".to_string())
        .code(FunctionCode::Image {
            image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/private:latest".to_string(),
        })
        .permissions("default-profile".to_string())
        .ingress(Ingress::Private)
        .build()
}

#[fixture]
pub(crate) fn function_with_concurrency() -> Function {
    Function::new("concurrent-func".to_string())
        .code(FunctionCode::Image {
            image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/concurrent:latest".to_string(),
        })
        .permissions("default-profile".to_string())
        .concurrency_limit(100)
        .build()
}

#[fixture]
pub(crate) fn function_custom_config() -> Function {
    Function::new("custom-func".to_string())
        .code(FunctionCode::Image {
            image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/custom:latest".to_string(),
        })
        .permissions("default-profile".to_string())
        .memory_mb(512)
        .timeout_seconds(120)
        .build()
}

#[fixture]
pub(crate) fn function_with_readiness_probe() -> Function {
    Function::new("probe-func".to_string())
        .code(FunctionCode::Image {
            image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/probe:latest".to_string(),
        })
        .permissions("default-profile".to_string())
        .ingress(Ingress::Public)
        .readiness_probe(ReadinessProbe {
            method: HttpMethod::Get,
            path: "/health/ready".to_string(),
        })
        .build()
}

#[fixture]
pub(crate) fn function_complete_test() -> Function {
    let mut env_vars = HashMap::new();
    env_vars.insert("ENV_VAR_1".to_string(), "value1".to_string());
    env_vars.insert("ENV_VAR_2".to_string(), "value2".to_string());

    Function::new("test-function".to_string())
        .code(FunctionCode::Image {
            image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/my-repo:latest".to_string(),
        })
        .timeout_seconds(30)
        .memory_mb(128)
        .environment(env_vars)
        .permissions("default-profile".to_string())
        .ingress(Ingress::Public)
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
pub(crate) fn function_with_queue_trigger() -> Function {
    let queue = test_queue();

    Function::new("queue-func".to_string())
        .code(FunctionCode::Image {
            image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/queue:latest".to_string(),
        })
        .permissions("default-profile".to_string())
        .trigger(FunctionTrigger::queue(&queue))
        .build()
}
