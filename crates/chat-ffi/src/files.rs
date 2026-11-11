//! Sending images and files from the phone.

use std::path::PathBuf;

use chat_core::{SendRequest, UploadRequest};

use crate::{ChatClient, CoreError};

#[uniffi::export]
impl ChatClient {
    /// Uploads a local file and sends it as a message with an optional caption. Waits for the
    /// upload, so call it off the main thread; the message then goes through the outbox like
    /// any other. Returns the message's client id.
    pub fn send_file(
        &self,
        group_id: String,
        topic_id: String,
        path: String,
        name: Option<String>,
        caption: String,
        quote_id: Option<String>,
    ) -> Result<String, CoreError> {
        let path = PathBuf::from(path);
        if !path.is_file() {
            return Err(CoreError::Invalid);
        }
        let task = self.inner.upload_file(
            UploadRequest {
                group_id: group_id.clone(),
                topic_id: Some(topic_id.clone()),
                path,
                name,
                mime: None,
            },
            chat_core::no_progress(),
        );
        let meta = chat_core::client::background()
            .block_on(task)
            .map_err(|e| CoreError::Failed {
                message: e.to_string(),
            })??;
        Ok(self.inner.send(SendRequest {
            group_id,
            topic_id: Some(topic_id),
            body: caption,
            reply_to: quote_id,
            attachments: Some(vec![meta.to_file_ref()]),
            ..Default::default()
        })?)
    }
}
