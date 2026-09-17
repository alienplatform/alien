use std::{
    any::{type_name, Any, TypeId},
    collections::HashMap,
    fmt,
    sync::Arc,
};

use alien_error::AlienError;

use crate::error::{ErrorData, Result};

struct ServiceEntry {
    type_name: &'static str,
    service: Box<dyn Any + Send + Sync>,
}

/// Type-indexed provider services available to resource controllers.
///
/// The registry stores `Arc<T>` as a sized value, which allows `T` itself to
/// be a trait object. Provider-neutral controller code therefore does not need
/// to import any concrete provider client types.
#[derive(Default)]
pub struct ServiceRegistry {
    services: HashMap<TypeId, ServiceEntry>,
}

impl fmt::Debug for ServiceRegistry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut service_types = self
            .services
            .values()
            .map(|entry| entry.type_name)
            .collect::<Vec<_>>();
        service_types.sort_unstable();

        formatter
            .debug_struct("ServiceRegistry")
            .field("service_types", &service_types)
            .finish()
    }
}

impl ServiceRegistry {
    /// Creates an empty service registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers one concrete or trait-object service type.
    ///
    /// Registration is exact by `TypeId`; registering the same service type a
    /// second time is an error rather than silently changing controller
    /// behavior according to initialization order.
    pub fn register<T>(&mut self, service: Arc<T>) -> Result<()>
    where
        T: ?Sized + Send + Sync + 'static,
    {
        let service_type = TypeId::of::<T>();
        if self.services.contains_key(&service_type) {
            return Err(AlienError::new(ErrorData::ServiceAlreadyRegistered {
                service_type: type_name::<T>().to_string(),
            }));
        }

        self.services.insert(
            service_type,
            ServiceEntry {
                type_name: type_name::<T>(),
                service: Box::new(service),
            },
        );
        Ok(())
    }

    /// Returns a registered service, if present.
    pub fn get<T>(&self) -> Option<Arc<T>>
    where
        T: ?Sized + Send + Sync + 'static,
    {
        self.services
            .get(&TypeId::of::<T>())?
            .service
            .downcast_ref::<Arc<T>>()
            .cloned()
    }

    /// Returns a required service or a structured configuration error.
    pub fn require<T>(&self) -> Result<Arc<T>>
    where
        T: ?Sized + Send + Sync + 'static,
    {
        self.get::<T>().ok_or_else(|| {
            AlienError::new(ErrorData::ServiceNotRegistered {
                service_type: type_name::<T>().to_string(),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    trait ExampleService: Send + Sync {
        fn value(&self) -> u32;
    }

    struct ExampleServiceImpl(u32);

    impl ExampleService for ExampleServiceImpl {
        fn value(&self) -> u32 {
            self.0
        }
    }

    #[test]
    fn stores_and_retrieves_trait_object_services() {
        let mut registry = ServiceRegistry::new();
        let service: Arc<dyn ExampleService> = Arc::new(ExampleServiceImpl(42));

        registry
            .register(service)
            .expect("registration should succeed");

        assert_eq!(
            registry
                .require::<dyn ExampleService>()
                .expect("service should be registered")
                .value(),
            42
        );
    }

    #[test]
    fn rejects_duplicate_service_types() {
        let mut registry = ServiceRegistry::new();
        let first: Arc<dyn ExampleService> = Arc::new(ExampleServiceImpl(1));
        let second: Arc<dyn ExampleService> = Arc::new(ExampleServiceImpl(2));
        registry
            .register(first)
            .expect("first registration should succeed");

        let error = registry
            .register(second)
            .expect_err("duplicate service type should fail");

        assert_eq!(error.code, "SERVICE_ALREADY_REGISTERED");
        assert_eq!(
            registry
                .require::<dyn ExampleService>()
                .expect("first service should remain registered")
                .value(),
            1
        );
    }

    #[test]
    fn reports_missing_required_service() {
        let error = match ServiceRegistry::new().require::<dyn ExampleService>() {
            Ok(_) => panic!("missing service should fail"),
            Err(error) => error,
        };

        assert_eq!(error.code, "SERVICE_NOT_REGISTERED");
        assert!(error.to_string().contains("ExampleService"));
    }
}
