use crate::error::map_alien_error;
use alien_bindings::Key;
use napi::bindgen_prelude::Buffer;
use napi_derive::napi;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

#[napi]
pub struct KeyHandle {
    inner: Arc<dyn Key>,
}

impl KeyHandle {
    pub(crate) fn new(inner: Arc<dyn Key>) -> Self {
        Self { inner }
    }
}

fn ordered_context(context: Option<HashMap<String, String>>) -> Option<BTreeMap<String, String>> {
    context.map(|values| values.into_iter().collect())
}

#[napi]
impl KeyHandle {
    #[napi]
    pub async fn encrypt(
        &self,
        plaintext: Buffer,
        context: Option<HashMap<String, String>>,
    ) -> napi::Result<Buffer> {
        let context = ordered_context(context);
        self.inner
            .encrypt(&plaintext, context.as_ref())
            .await
            .map(Buffer::from)
            .map_err(map_alien_error)
    }

    #[napi]
    pub async fn decrypt(
        &self,
        ciphertext: Buffer,
        context: Option<HashMap<String, String>>,
    ) -> napi::Result<Buffer> {
        let context = ordered_context(context);
        self.inner
            .decrypt(&ciphertext, context.as_ref())
            .await
            .map(Buffer::from)
            .map_err(map_alien_error)
    }
}
