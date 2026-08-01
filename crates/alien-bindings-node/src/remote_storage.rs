//! Remote Storage v0 handle. The native surface mirrors the five authorized
//! operations and cannot expose the wider local `StorageHandle` API.

use crate::error::map_object_store_error;
use crate::storage::{object_meta_to_js, ObjectMetaJs};
use alien_bindings::RemoteStorage;
use futures::StreamExt;
use napi::bindgen_prelude::Buffer;
use napi_derive::napi;
use object_store::path::Path;
use object_store::PutPayload;
use std::sync::Arc;

#[napi]
pub struct RemoteStorageHandle {
    inner: Arc<dyn RemoteStorage>,
    binding: String,
}

impl RemoteStorageHandle {
    pub(crate) fn new(inner: Arc<dyn RemoteStorage>, binding: String) -> Self {
        Self { inner, binding }
    }
}

#[napi]
impl RemoteStorageHandle {
    #[napi]
    pub async fn get(&self, path: String) -> napi::Result<Buffer> {
        let result = self
            .inner
            .get(&Path::from(path))
            .await
            .map_err(|error| map_object_store_error(error, &self.binding, "get"))?;
        let bytes = result
            .bytes()
            .await
            .map_err(|error| map_object_store_error(error, &self.binding, "get"))?;
        Ok(Buffer::from(bytes.to_vec()))
    }

    #[napi]
    pub async fn put(&self, path: String, data: Buffer) -> napi::Result<()> {
        self.inner
            .put(&Path::from(path), PutPayload::from(data.to_vec()))
            .await
            .map_err(|error| map_object_store_error(error, &self.binding, "put"))?;
        Ok(())
    }

    #[napi]
    pub async fn delete(&self, path: String) -> napi::Result<()> {
        self.inner
            .delete(&Path::from(path))
            .await
            .map_err(|error| map_object_store_error(error, &self.binding, "delete"))
    }

    #[napi]
    pub async fn list(&self, prefix: Option<String>) -> napi::Result<Vec<ObjectMetaJs>> {
        let prefix = prefix.map(Path::from);
        let mut stream = self.inner.list(prefix.as_ref());
        let mut objects = Vec::new();
        while let Some(item) = stream.next().await {
            objects.push(object_meta_to_js(&item.map_err(|error| {
                map_object_store_error(error, &self.binding, "list")
            })?));
        }
        Ok(objects)
    }

    #[napi]
    pub async fn head(&self, path: String) -> napi::Result<ObjectMetaJs> {
        self.inner
            .head(&Path::from(path))
            .await
            .map(|metadata| object_meta_to_js(&metadata))
            .map_err(|error| map_object_store_error(error, &self.binding, "head"))
    }
}
