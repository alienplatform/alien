//! Remote Storage v0 handle. The native surface mirrors the five authorized
//! operations and cannot expose the wider local `StorageHandle` API.

use crate::error::map_object_store_error;
use crate::storage::{
    object_attributes_to_js, object_meta_to_js, object_store_put_options, put_result_to_js,
    ObjectMetaJs, StorageGetResultJs, StorageHeadResultJs, StoragePutOptionsJs, StoragePutResultJs,
};
use alien_bindings::RemoteStorage;
use futures::StreamExt;
use napi::bindgen_prelude::Buffer;
use napi_derive::napi;
use object_store::path::Path;
use object_store::{GetOptions, PutPayload};
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
    pub async fn get(&self, path: String) -> napi::Result<StorageGetResultJs> {
        let result = self
            .inner
            .get(&Path::from(path))
            .await
            .map_err(|error| map_object_store_error(error, &self.binding, "get"))?;
        let meta = object_meta_to_js(&result.meta);
        let attributes = object_attributes_to_js(&result.attributes);
        let data = result
            .bytes()
            .await
            .map_err(|error| map_object_store_error(error, &self.binding, "get"))?;
        Ok(StorageGetResultJs {
            data: Buffer::from(data.to_vec()),
            meta,
            attributes,
        })
    }

    #[napi]
    pub async fn put(
        &self,
        path: String,
        data: Buffer,
        options: Option<StoragePutOptionsJs>,
    ) -> napi::Result<StoragePutResultJs> {
        let path = Path::from(path);
        let payload = PutPayload::from(data.to_vec());
        let result = match options {
            Some(options) => {
                self.inner
                    .put_opts(&path, payload, object_store_put_options(options))
                    .await
            }
            None => self.inner.put(&path, payload).await,
        }
        .map_err(|error| map_object_store_error(error, &self.binding, "put"))?;
        Ok(put_result_to_js(result))
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
    pub async fn head(&self, path: String) -> napi::Result<StorageHeadResultJs> {
        let result = self
            .inner
            .get_opts(
                &Path::from(path),
                GetOptions {
                    head: true,
                    ..Default::default()
                },
            )
            .await
            .map_err(|error| map_object_store_error(error, &self.binding, "head"))?;
        Ok(StorageHeadResultJs {
            meta: object_meta_to_js(&result.meta),
            attributes: object_attributes_to_js(&result.attributes),
        })
    }
}
