#![cfg(feature = "grpc")]

use crate::BindingsProviderApi;
use alien_error::AlienError;
use async_trait::async_trait;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{oneshot, Mutex};
use tonic::{Request, Response, Status};
use tracing::{debug, info, warn};

// Module for the generated gRPC code.
pub mod alien_bindings {
    pub mod wait_until {
        tonic::include_proto!("alien_bindings.wait_until");
        pub const FILE_DESCRIPTOR_SET: &[u8] =
            tonic::include_file_descriptor_set!("alien_bindings.wait_until_descriptor");
    }
}

use alien_bindings::wait_until::{
    wait_until_service_server::{WaitUntilService, WaitUntilServiceServer},
    GetTaskCountRequest, GetTaskCountResponse, NotifyDrainCompleteRequest,
    NotifyDrainCompleteResponse, NotifyTaskRegisteredRequest, NotifyTaskRegisteredResponse,
    WaitForDrainSignalRequest, WaitForDrainSignalResponse,
};

/// Represents an application that has registered tasks and may be waiting for drain signals.
#[derive(Debug)]
struct ApplicationState {
    /// Number of currently registered tasks.
    task_count: u32,
    /// Channel to send drain signal to the application when it's time to drain.
    drain_signal_sender: Option<oneshot::Sender<WaitForDrainSignalResponse>>,
}

#[derive(Clone)]
pub struct WaitUntilGrpcServer {
    provider: Arc<dyn BindingsProviderApi>,
    /// Track application states by application_id.
    applications: Arc<Mutex<HashMap<String, ApplicationState>>>,
}

impl WaitUntilGrpcServer {
    pub fn new(provider: Arc<dyn BindingsProviderApi>) -> Self {
        Self {
            provider,
            applications: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn into_service(self) -> WaitUntilServiceServer<Self> {
        WaitUntilServiceServer::new(self)
    }

    /// Trigger drain for all registered applications.
    /// This is called by the runtime when it's time to drain (e.g., on SIGTERM or Lambda INVOKE end).
    pub async fn trigger_drain_all(
        &self,
        reason: &str,
        timeout_secs: u64,
    ) -> Result<(), AlienError> {
        let mut applications = self.applications.lock().await;

        info!("Triggering drain for {} applications", applications.len());

        for (app_id, app_state) in applications.iter_mut() {
            if let Some(sender) = app_state.drain_signal_sender.take() {
                let response = WaitForDrainSignalResponse {
                    should_drain: true,
                    drain_timeout: Some(prost_types::Duration {
                        seconds: timeout_secs as i64,
                        nanos: 0,
                    }),
                    drain_reason: reason.to_string(),
                };

                if let Err(_) = sender.send(response) {
                    warn!("Failed to send drain signal to application {}", app_id);
                }
            }
        }

        Ok(())
    }

    /// Get total number of tasks across all applications.
    pub async fn get_total_task_count(&self) -> u32 {
        let applications = self.applications.lock().await;
        applications.values().map(|app| app.task_count).sum()
    }
}

#[async_trait]
impl WaitUntilService for WaitUntilGrpcServer {
    async fn notify_task_registered(
        &self,
        request: Request<NotifyTaskRegisteredRequest>,
    ) -> Result<Response<NotifyTaskRegisteredResponse>, Status> {
        let req = request.into_inner();
        let app_id = req.application_id;

        debug!(
            app_id = %app_id,
            task_description = %req.task_description.as_deref().unwrap_or_default(),
            "Task registered"
        );

        let mut applications = self.applications.lock().await;
        let app_state = applications
            .entry(app_id.clone())
            .or_insert_with(|| ApplicationState {
                task_count: 0,
                drain_signal_sender: None,
            });

        app_state.task_count += 1;

        debug!(app_id = %app_id, task_count = app_state.task_count, "Updated task count");

        Ok(Response::new(NotifyTaskRegisteredResponse {
            success: true,
        }))
    }

    async fn wait_for_drain_signal(
        &self,
        request: Request<WaitForDrainSignalRequest>,
    ) -> Result<Response<WaitForDrainSignalResponse>, Status> {
        let req = request.into_inner();
        let app_id = req.application_id;

        debug!(app_id = %app_id, "Application waiting for drain signal");

        let (sender, receiver) = oneshot::channel();

        // Store the sender in the application state
        {
            let mut applications = self.applications.lock().await;
            let app_state =
                applications
                    .entry(app_id.clone())
                    .or_insert_with(|| ApplicationState {
                        task_count: 0,
                        drain_signal_sender: None,
                    });
            app_state.drain_signal_sender = Some(sender);
        }

        // Wait for the drain signal
        match receiver.await {
            Ok(response) => {
                debug!(app_id = %app_id, reason = %response.drain_reason, "Sending drain signal to application");
                Ok(Response::new(response))
            }
            Err(_) => {
                // Channel was dropped, likely due to shutdown
                warn!(app_id = %app_id, "Drain signal channel dropped");
                Ok(Response::new(WaitForDrainSignalResponse {
                    should_drain: true,
                    drain_timeout: Some(prost_types::Duration {
                        seconds: 10,
                        nanos: 0,
                    }),
                    drain_reason: "runtime_shutdown".to_string(),
                }))
            }
        }
    }

    async fn notify_drain_complete(
        &self,
        request: Request<NotifyDrainCompleteRequest>,
    ) -> Result<Response<NotifyDrainCompleteResponse>, Status> {
        let req = request.into_inner();
        let app_id = req.application_id;

        info!(
            app_id = %app_id,
            tasks_drained = req.tasks_drained,
            success = req.success,
            error = %req.error_message.as_deref().unwrap_or_default(),
            "Application completed draining"
        );

        // Update the application state to reflect that tasks have been drained
        {
            let mut applications = self.applications.lock().await;
            if let Some(app_state) = applications.get_mut(&app_id) {
                app_state.task_count = app_state.task_count.saturating_sub(req.tasks_drained);
            }
        }

        Ok(Response::new(NotifyDrainCompleteResponse {
            acknowledged: true,
        }))
    }

    async fn get_task_count(
        &self,
        request: Request<GetTaskCountRequest>,
    ) -> Result<Response<GetTaskCountResponse>, Status> {
        let req = request.into_inner();
        let app_id = req.application_id;

        let applications = self.applications.lock().await;
        let task_count = applications
            .get(&app_id)
            .map(|app| app.task_count)
            .unwrap_or(0);

        Ok(Response::new(GetTaskCountResponse { task_count }))
    }
}
