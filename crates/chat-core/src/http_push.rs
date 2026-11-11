//! Phone push endpoints (docs/protocol.md, "Push"): device registration, the 等你 badge and
//! answering a card from a notification without a socket.

use crate::http::{ApiError, ApiResult, HttpClient, enc};
use crate::proto::{
    AnswerCardRequest, BadgeResponse, CardEventAck, PushDevice, PushDevicesResponse,
    RegisterPushDeviceRequest, RemovePushDeviceResponse,
};
use reqwest::Method;

fn invalid(e: crate::proto::ValidationError) -> ApiError {
    ApiError::Invalid(e.to_string())
}

impl HttpClient {
    /// This sign-in's phone wants pushes. Registering a known token moves it here.
    pub async fn register_push_device(
        &self,
        req: &RegisterPushDeviceRequest,
    ) -> ApiResult<PushDevice> {
        req.validate().map_err(invalid)?;
        self.call(Method::POST, "/v1/me/push-devices", Some(req), true)
            .await
    }

    /// My registered phones.
    pub async fn push_devices(&self) -> ApiResult<Vec<PushDevice>> {
        let r: PushDevicesResponse = self
            .call::<(), _>(Method::GET, "/v1/me/push-devices", None, true)
            .await?;
        Ok(r.devices)
    }

    /// Removes the devices of this sign-in (push off, signing out). Returns how many.
    pub async fn remove_push_devices(&self) -> ApiResult<u64> {
        let r: RemovePushDeviceResponse = self
            .call::<(), _>(Method::DELETE, "/v1/me/push-devices", None, true)
            .await?;
        Ok(r.removed)
    }

    /// Topics that wait on me (等你): the app icon's badge.
    pub async fn badge(&self) -> ApiResult<u64> {
        let r: BadgeResponse = self
            .call::<(), _>(Method::GET, "/v1/me/badge", None, true)
            .await?;
        Ok(r.waiting)
    }

    /// `card.answer` over HTTP, for a notification action.
    pub async fn answer_card(
        &self,
        group_id: &str,
        message_id: &str,
        req: &AnswerCardRequest,
    ) -> ApiResult<CardEventAck> {
        req.validate().map_err(invalid)?;
        self.call(
            Method::POST,
            &format!(
                "/v1/groups/{}/messages/{}/answer",
                enc(group_id),
                enc(message_id)
            ),
            Some(req),
            true,
        )
        .await
    }
}
