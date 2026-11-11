//! 我: my signed-in devices, my password and code sign-in for other devices. Each call waits
//! for the server, so call them off the main thread.

use chat_core::proto::{QrOfferState, SignInLink};

use crate::{ChatClient, CoreError};

fn block_on<T>(f: impl std::future::Future<Output = T>) -> T {
    chat_core::client::background().block_on(f)
}

/// One signed-in device: a live token of mine.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DeviceItem {
    pub id: String,
    /// The name given at sign in; may be empty.
    pub name: String,
    /// `phone`, `laptop` or `desktop`, guessed from the name, for the symbol.
    pub kind: String,
    pub created_at: u64,
    pub last_used_at: Option<u64>,
    /// This device.
    pub current: bool,
}

/// The symbol a device name suggests.
pub fn device_kind(name: &str) -> &'static str {
    let n = name.to_lowercase();
    if ["iphone", "ipad", "android", "phone", "手机"]
        .iter()
        .any(|k| n.contains(k))
    {
        "phone"
    } else if [
        "imac",
        "mac mini",
        "mac studio",
        "mac pro",
        "desktop",
        "台式",
    ]
    .iter()
    .any(|k| n.contains(k))
    {
        "desktop"
    } else {
        "laptop"
    }
}

/// Whether I have a password, and since when.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PasswordState {
    pub set: bool,
    pub updated_at: Option<u64>,
}

/// A code this device shows so a new device signs in to my account.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SignInOffer {
    pub id: String,
    pub code: String,
    pub qr_text: String,
    pub expires_at: u64,
}

/// `waiting`, `used` (with the device that signed in) or `expired`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum OfferState {
    Waiting,
    Used { device_name: String },
    Expired,
}

#[uniffi::export]
impl ChatClient {
    /// My devices, newest use first.
    pub fn devices(&self) -> Result<Vec<DeviceItem>, CoreError> {
        let http = self.inner.http();
        let r = block_on(async move { http.devices().await })?;
        Ok(r.devices
            .into_iter()
            .map(|d| DeviceItem {
                kind: device_kind(&d.name).into(),
                id: d.id,
                name: d.name,
                created_at: d.created_at,
                last_used_at: d.last_used_at,
                current: d.current,
            })
            .collect())
    }

    /// Signs one of my devices out: its sign-in stops working at once.
    pub fn sign_out_device(&self, id: String) -> Result<(), CoreError> {
        let http = self.inner.http();
        block_on(async move { http.sign_out_device(&id).await })?;
        Ok(())
    }

    /// My sign-in email, if any (password sign-in needs one).
    pub fn my_email(&self) -> Result<Option<String>, CoreError> {
        let http = self.inner.http();
        Ok(block_on(async move { http.my_email().await })?.email)
    }

    pub fn password_state(&self) -> Result<PasswordState, CoreError> {
        let http = self.inner.http();
        let r = block_on(async move { http.my_password().await })?;
        Ok(PasswordState {
            set: r.set,
            updated_at: r.updated_at,
        })
    }

    /// Sets or changes my password. A missing or wrong `current` is [`CoreError::Invalid`].
    pub fn set_password(
        &self,
        password: String,
        current: Option<String>,
    ) -> Result<PasswordState, CoreError> {
        let http = self.inner.http();
        let r = block_on(async move { http.set_password(&password, current.as_deref()).await })?;
        Ok(PasswordState {
            set: r.set,
            updated_at: r.updated_at,
        })
    }

    pub fn remove_password(&self, current: Option<String>) -> Result<(), CoreError> {
        let http = self.inner.http();
        block_on(async move { http.remove_password(current.as_deref()).await })?;
        Ok(())
    }

    /// A code to show so a new device signs in to my account.
    pub fn offer_sign_in(&self) -> Result<SignInOffer, CoreError> {
        let http = self.inner.http();
        let base = http.base().to_string();
        let r = block_on(async move { http.qr_offer().await })?;
        Ok(SignInOffer {
            qr_text: SignInLink {
                code: r.code.clone(),
                server: Some(base),
            }
            .to_text(),
            id: r.id,
            code: r.code,
            expires_at: r.expires_at,
        })
    }

    pub fn offer_state(&self, id: String) -> Result<OfferState, CoreError> {
        let http = self.inner.http();
        let r = block_on(async move { http.qr_offer_status(&id).await })?;
        Ok(match r.state {
            QrOfferState::Waiting => OfferState::Waiting,
            QrOfferState::Used => OfferState::Used {
                device_name: r.device_name.unwrap_or_default(),
            },
            QrOfferState::Expired => OfferState::Expired,
        })
    }

    /// The name of the device that shows `code`, before approving it. A wrong, used or
    /// expired code is [`CoreError::NotFound`].
    pub fn peek_sign_in(&self, code: String) -> Result<String, CoreError> {
        let http = self.inner.http();
        Ok(block_on(async move { http.qr_peek(&code).await })?.device_name)
    }

    /// Lets the device that shows `code` in to my account.
    pub fn approve_sign_in(&self, code: String) -> Result<(), CoreError> {
        let http = self.inner.http();
        block_on(async move { http.qr_approve(&code).await })?;
        Ok(())
    }
}
