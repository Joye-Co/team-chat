//! V1 operations on [`Client`] that combine HTTP with the cache. Each async method runs
//! on the core runtime and returns a `JoinHandle` that any executor can await; abort
//! the handle to cancel a transfer. Plain HTTP calls (audit, hooks, invites) are on
//! [`crate::HttpClient`]: `client.spawn(async move { http.audit(None, None).await })`.

use crate::artifacts::{self, NewVersion};
use crate::client::{Client, ClientError, ClientUpdate};
use crate::files::{FileCache, ProgressFn, UploadRequest};
use crate::http::ApiError;
use crate::proto::machine::RunLogEntry;
use crate::proto::{
    Artifact, ArtifactVersionResponse, BotRun, FileMeta, FileRef, Member, UpdateGroupPrefsRequest,
    UpdateInfo, UpdateMemberRequest,
};
use crate::store::{ArtifactRow, CardActionRow, GroupRow, TodoRow};
use std::path::{Path, PathBuf};
use tokio::task::JoinHandle;

/// A 产出 version on disk, ready for the 产出 window.
/// What [`Client::cancel_run`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelRun {
    /// The run was active and is now `cancelled`.
    Cancelled,
    /// The run had already ended (`conflict`); the cache now has its end.
    AlreadyEnded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactFolder {
    pub artifact_id: String,
    pub version: u32,
    /// The version folder.
    pub dir: PathBuf,
    /// `dir` joined with the version's entry path.
    pub entry: PathBuf,
}

impl Client {
    fn file_cache(&self) -> FileCache {
        FileCache::new(
            self.store().clone(),
            self.config().files_dir(),
            self.config().file_cache_max_bytes,
        )
    }

    // ------------------------------------------------------------------
    // Cache reads (synchronous)
    // ------------------------------------------------------------------

    /// 待办: topics that wait on me (not done) and topics with unread mentions.
    pub fn todo(&self) -> Result<Vec<TodoRow>, ClientError> {
        Ok(self.store().todo()?)
    }

    /// 产出 of a topic, newest first.
    pub fn artifacts(&self, topic_id: &str) -> Result<Vec<Artifact>, ClientError> {
        Ok(self.store().artifacts(topic_id)?)
    }

    /// A cached 产出 with its cached versions, newest first.
    pub fn artifact(&self, artifact_id: &str) -> Result<Option<ArtifactRow>, ClientError> {
        Ok(self.store().artifact(artifact_id)?)
    }

    /// Who pressed which button of a card message, oldest first.
    pub fn card_actions(&self, message_id: &str) -> Result<Vec<CardActionRow>, ClientError> {
        Ok(self.store().card_actions(message_id)?)
    }

    /// The cached copy of a file under its own name, without network.
    pub fn cached_file(&self, file: &FileRef) -> Option<PathBuf> {
        self.file_cache().get_named(&file.sha256, &file.name)
    }

    /// The folder of a 产出 version when it is on disk, without network.
    pub fn cached_artifact_version(&self, artifact_id: &str, version: u32) -> Option<PathBuf> {
        artifacts::cached_version_dir(&self.config().artifacts_dir(), artifact_id, version)
    }

    // ------------------------------------------------------------------
    // Members and preferences
    // ------------------------------------------------------------------

    /// Sets my mute and notify level for a group, and stores the new summary.
    pub fn set_group_prefs(
        &self,
        group_id: &str,
        req: UpdateGroupPrefsRequest,
    ) -> JoinHandle<Result<GroupRow, ClientError>> {
        let http = self.http();
        let shared = self.shared().clone();
        let gid = group_id.to_string();
        self.spawn(async move {
            let summary = http.update_group_prefs(&gid, &req).await?;
            shared
                .store
                .apply_directory(None, Some(std::slice::from_ref(&summary)), None)?;
            shared.emit(ClientUpdate::GroupChanged {
                group_id: gid.clone(),
            });
            shared.store.group(&gid)?.ok_or(ClientError::NotFound)
        })
    }

    /// hide-delete/1: hides or shows a 群 or 私聊 for me (`PATCH /v1/groups/:id/prefs` with
    /// `hidden`). It comes back by itself with the next message from someone else.
    pub fn set_group_hidden(
        &self,
        group_id: &str,
        hidden: bool,
    ) -> JoinHandle<Result<GroupRow, ClientError>> {
        self.set_group_prefs(
            group_id,
            UpdateGroupPrefsRequest {
                hidden: Some(hidden),
                ..Default::default()
            },
        )
    }

    /// hide-delete/1: deletes a 群 for everyone (`DELETE /v1/groups/:id`; an owner, not a DM,
    /// not the default group). The group leaves the cache at once; the server's `directory`
    /// says the same to every member.
    pub fn delete_group(&self, group_id: &str) -> JoinHandle<Result<(), ClientError>> {
        let http = self.http();
        let shared = self.shared().clone();
        let gid = group_id.to_string();
        self.spawn(async move {
            http.delete_group(&gid).await?;
            shared
                .store
                .apply_directory(None, None, Some(std::slice::from_ref(&gid)))?;
            shared.emit(ClientUpdate::DirectoryChanged);
            Ok(())
        })
    }

    /// hide-delete/1: deletes a deactivated AI member for good (`DELETE /v1/members/:id`; an
    /// owner). The member stays in the cache with `deleted` so old messages keep their author;
    /// lists leave it out.
    pub fn delete_member(&self, member_id: &str) -> JoinHandle<Result<Member, ClientError>> {
        let http = self.http();
        let shared = self.shared().clone();
        let id = member_id.to_string();
        self.spawn(async move {
            let m = http.delete_member(&id).await?;
            shared
                .store
                .apply_directory(Some(std::slice::from_ref(&m)), None, None)?;
            shared.emit(ClientUpdate::DirectoryChanged);
            Ok(m)
        })
    }

    /// Renames a group (`PATCH /v1/groups/:id`). The server appends `group.updated`, which
    /// updates every client; the answer is stored at once too when I am in the group, so the
    /// sidebar does not wait for the event. Answers with the stored name.
    pub fn rename_group(
        &self,
        group_id: &str,
        name: &str,
    ) -> JoinHandle<Result<String, ClientError>> {
        let http = self.http();
        let shared = self.shared().clone();
        let (gid, name) = (group_id.to_string(), name.to_string());
        self.spawn(async move {
            let summary = http.rename_group(&gid, &name).await?;
            let me = shared.store.me()?.map(|m| m.id).unwrap_or_default();
            if summary.group.member_ids.contains(&me) {
                shared
                    .store
                    .apply_directory(None, Some(std::slice::from_ref(&summary)), None)?;
                shared.emit(ClientUpdate::GroupChanged {
                    group_id: gid.clone(),
                });
            }
            Ok(summary.group.name)
        })
    }

    /// 1.1: sets a group's purpose line (`PATCH /v1/groups/:id` with `purpose`; `""` clears
    /// it). Same rights and storing as [`Client::rename_group`]. Answers with the stored
    /// purpose.
    pub fn set_group_purpose(
        &self,
        group_id: &str,
        purpose: &str,
    ) -> JoinHandle<Result<Option<String>, ClientError>> {
        let http = self.http();
        let shared = self.shared().clone();
        let (gid, purpose) = (group_id.to_string(), purpose.to_string());
        self.spawn(async move {
            let summary = http.set_group_purpose(&gid, &purpose).await?;
            let me = shared.store.me()?.map(|m| m.id).unwrap_or_default();
            if summary.group.member_ids.contains(&me) {
                shared
                    .store
                    .apply_directory(None, Some(std::slice::from_ref(&summary)), None)?;
                shared.emit(ClientUpdate::GroupChanged {
                    group_id: gid.clone(),
                });
            }
            Ok(summary.group.purpose)
        })
    }

    /// Changes a member (owner: every field; self: name and role) and stores it.
    pub fn update_member(
        &self,
        member_id: &str,
        req: UpdateMemberRequest,
    ) -> JoinHandle<Result<Member, ClientError>> {
        let http = self.http();
        let shared = self.shared().clone();
        let id = member_id.to_string();
        self.spawn(async move {
            let m = http.update_member(&id, &req).await?;
            store_member(&shared, &m)?;
            Ok(m)
        })
    }

    /// Uploads an avatar image (png, jpeg or webp, at most 1 MiB) and stores the member.
    pub fn upload_avatar(
        &self,
        member_id: &str,
        bytes: Vec<u8>,
        mime: &str,
    ) -> JoinHandle<Result<Member, ClientError>> {
        let http = self.http();
        let shared = self.shared().clone();
        let id = member_id.to_string();
        let mime = mime.to_string();
        self.spawn(async move {
            let m = http.upload_avatar(&id, bytes, &mime).await?;
            store_member(&shared, &m)?;
            Ok(m)
        })
    }

    /// Removes a member's avatar and stores the member.
    pub fn remove_avatar(&self, member_id: &str) -> JoinHandle<Result<Member, ClientError>> {
        let http = self.http();
        let shared = self.shared().clone();
        let id = member_id.to_string();
        self.spawn(async move {
            let m = http.remove_avatar(&id).await?;
            store_member(&shared, &m)?;
            Ok(m)
        })
    }

    /// A member's avatar image on disk (`avatars/<sha256 of the URL>`), downloaded
    /// once per `avatarUrl` (the URL changes with each new avatar). `None` when the
    /// member has no avatar.
    pub fn avatar(&self, member: &Member) -> JoinHandle<Result<Option<PathBuf>, ClientError>> {
        let http = self.http();
        let dir = self.config().avatars_dir();
        let url = member.avatar_url.clone().flatten();
        self.spawn(async move {
            let Some(url) = url else { return Ok(None) };
            let path = avatar_path(&dir, &url);
            if path.is_file() {
                return Ok(Some(path));
            }
            let bytes = http.avatar_bytes(&url).await?;
            tokio::fs::create_dir_all(&dir)
                .await
                .map_err(ApiError::from)?;
            let part = dir.join(format!(".part-{}", crate::new_id()));
            tokio::fs::write(&part, &bytes)
                .await
                .map_err(ApiError::from)?;
            tokio::fs::rename(&part, &path)
                .await
                .map_err(ApiError::from)?;
            Ok(Some(path))
        })
    }

    /// The avatar image of a member when it is on disk already, without network. Download
    /// it with [`Client::avatar`].
    pub fn cached_avatar(&self, member: &Member) -> Option<PathBuf> {
        let url = member.avatar_url()?;
        let path = avatar_path(&self.config().avatars_dir(), url);
        path.is_file().then_some(path)
    }

    // ------------------------------------------------------------------
    // Files
    // ------------------------------------------------------------------

    /// Uploads a local file. Attach the result with `meta.to_file_ref()` in
    /// `SendRequest.attachments`. Images are also put in the file cache for previews.
    pub fn upload_file(
        &self,
        req: UploadRequest,
        progress: ProgressFn,
    ) -> JoinHandle<Result<FileMeta, ClientError>> {
        let http = self.http();
        let cache = self.file_cache();
        self.spawn(async move {
            let meta = crate::files::upload_file(&http, &req, &progress).await?;
            if meta.mime.starts_with("image/") {
                let _ = cache.insert_copy(&req.path, &meta.sha256, &meta.name).await;
            }
            Ok(meta)
        })
    }

    /// The file on disk: from the cache, or downloaded and checked against its SHA-256.
    pub fn download_file(
        &self,
        file: FileRef,
        progress: ProgressFn,
    ) -> JoinHandle<Result<PathBuf, ClientError>> {
        let http = self.http();
        let cache = self.file_cache();
        self.spawn(async move { Ok(cache.fetch(&http, &file, &progress).await?) })
    }

    // ------------------------------------------------------------------
    // 产出
    // ------------------------------------------------------------------

    /// Fetches the 产出 list of a group (or one topic) and stores it.
    pub fn refresh_artifacts(
        &self,
        group_id: &str,
        topic_id: Option<&str>,
    ) -> JoinHandle<Result<Vec<Artifact>, ClientError>> {
        let http = self.http();
        let shared = self.shared().clone();
        let gid = group_id.to_string();
        let tid = topic_id.map(str::to_string);
        self.spawn(async move {
            let list = http.artifacts(&gid, tid.as_deref()).await?;
            shared.store.store_artifacts(&list)?;
            let topics: std::collections::BTreeSet<&str> =
                list.iter().map(|a| a.topic_id.as_str()).collect();
            for t in topics {
                shared.emit(ClientUpdate::ArtifactsChanged {
                    group_id: gid.clone(),
                    topic_id: t.to_string(),
                });
            }
            Ok(list)
        })
    }

    /// Fetches a 产出 with all its versions and stores it.
    pub fn load_artifact(&self, artifact_id: &str) -> JoinHandle<Result<ArtifactRow, ClientError>> {
        let http = self.http();
        let shared = self.shared().clone();
        let id = artifact_id.to_string();
        self.spawn(async move { load_artifact(&http, &shared, &id).await })
    }

    /// Puts every file of a version (the current one when `version` is `None`) in its
    /// cache folder and returns the folder and entry file.
    pub fn open_artifact_version(
        &self,
        artifact_id: &str,
        version: Option<u32>,
        progress: ProgressFn,
    ) -> JoinHandle<Result<ArtifactFolder, ClientError>> {
        let http = self.http();
        let shared = self.shared().clone();
        let root = self.config().artifacts_dir();
        let id = artifact_id.to_string();
        self.spawn(async move {
            let cached = shared.store.artifact(&id)?;
            let want = version.or(cached.as_ref().map(|r| r.artifact.current_version));
            let mut v = match want {
                Some(n) => shared.store.artifact_version(&id, n)?,
                None => None,
            };
            if v.is_none() {
                let row = load_artifact(&http, &shared, &id).await?;
                let n = version.unwrap_or(row.artifact.current_version);
                v = row.versions.into_iter().find(|x| x.version == n);
            }
            let v = v.ok_or(ClientError::NotFound)?;
            let dir = artifacts::fetch_version(&http, &root, &v, &progress).await?;
            let entry = dir.join(v.entry.replace('/', std::path::MAIN_SEPARATOR_STR));
            Ok(ArtifactFolder {
                artifact_id: id,
                version: v.version,
                dir,
                entry,
            })
        })
    }

    // --- inc-host track ---
    /// Sends the media metadata of a 产出 version (`artifact.previewReady`, MEDIA §3).
    pub fn put_artifact_media(
        &self,
        artifact_id: &str,
        version: u32,
        media: crate::proto::MediaInfo,
    ) -> JoinHandle<Result<(), ClientError>> {
        let http = self.http();
        let id = artifact_id.to_string();
        self.spawn(async move {
            http.put_artifact_media(&id, version, &media).await?;
            Ok(())
        })
    }
    // --- end inc-host track ---

    /// Uploads the local files and makes a new 产出 or a new version; stores the result.
    pub fn create_artifact(
        &self,
        req: NewVersion,
        progress: ProgressFn,
    ) -> JoinHandle<Result<ArtifactVersionResponse, ClientError>> {
        let http = self.http();
        let shared = self.shared().clone();
        self.spawn(async move {
            let r = artifacts::create_version(&http, &req, &progress).await?;
            shared
                .store
                .store_artifact_response(&crate::proto::ArtifactResponse {
                    artifact: r.artifact.clone(),
                    versions: vec![r.version.clone()],
                })?;
            shared.emit(ClientUpdate::ArtifactsChanged {
                group_id: r.artifact.group_id.clone(),
                topic_id: r.artifact.topic_id.clone(),
            });
            Ok(r)
        })
    }

    // ------------------------------------------------------------------
    // Bot runs (process view)
    // ------------------------------------------------------------------

    /// Bot runs of a topic from the cache (`run.updated` events), newest first.
    pub fn runs(&self, topic_id: &str) -> Result<Vec<BotRun>, ClientError> {
        Ok(self.store().runs(topic_id)?)
    }

    /// The run log after `after_seq` (all pages, at most 20 x 500 entries) for the
    /// process view; feed it to [`crate::runs::ProcessView::apply`]. Also stores the
    /// newest run record. Returns (entries, last seq); call again with the last seq to
    /// follow a running run.
    pub fn run_log(
        &self,
        run_id: &str,
        after_seq: u64,
    ) -> JoinHandle<Result<(Vec<RunLogEntry>, u64), ClientError>> {
        let http = self.http();
        let shared = self.shared().clone();
        let id = run_id.to_string();
        self.spawn(async move {
            let (run, entries, last) = http.run_events_all(&id, after_seq, 20).await?;
            let changed = shared.store.run(&run.id)?.as_ref() != Some(&run);
            shared.store.store_run(&run)?;
            if changed {
                shared.emit(ClientUpdate::RunsChanged {
                    group_id: run.group_id.clone(),
                    topic_id: run.topic_id.clone(),
                });
            }
            Ok((entries, last))
        })
    }

    /// Stops an active run (a person in the run's group). Stores the ended run and emits
    /// [`ClientUpdate::RunsChanged`], so views show the end before the `run.updated` event
    /// arrives. A run that already ended is not an error: the client reads the run again,
    /// stores it and answers [`CancelRun::AlreadyEnded`].
    pub fn cancel_run(&self, run_id: &str) -> JoinHandle<Result<CancelRun, ClientError>> {
        let http = self.http();
        let shared = self.shared().clone();
        let id = run_id.to_string();
        self.spawn(async move {
            let (run, outcome) = match http.cancel_run(&id).await {
                Ok(run) => (run, CancelRun::Cancelled),
                Err(e) if e.is_conflict() => (http.run(&id).await?, CancelRun::AlreadyEnded),
                Err(e) => return Err(e.into()),
            };
            shared.store.store_run(&run)?;
            shared.emit(ClientUpdate::RunsChanged {
                group_id: run.group_id.clone(),
                topic_id: run.topic_id.clone(),
            });
            Ok(outcome)
        })
    }

    /// Usable: stops relay `relay_id` of topic `topic_id` (按顺序做 → 停止). Stores the
    /// answered relay in the topic and emits [`ClientUpdate::TopicsChanged`], so the strip
    /// shows the stop before the `topic.updated` event arrives.
    pub fn stop_relay(
        &self,
        group_id: &str,
        topic_id: &str,
        relay_id: &str,
    ) -> JoinHandle<Result<crate::proto::TopicRelay, ClientError>> {
        self.relay_action(group_id, topic_id, relay_id, false)
    }

    /// Usable: starts paused relay `relay_id` again (按顺序做 → 重试); stored as
    /// [`Client::stop_relay`] does.
    pub fn retry_relay(
        &self,
        group_id: &str,
        topic_id: &str,
        relay_id: &str,
    ) -> JoinHandle<Result<crate::proto::TopicRelay, ClientError>> {
        self.relay_action(group_id, topic_id, relay_id, true)
    }

    fn relay_action(
        &self,
        group_id: &str,
        topic_id: &str,
        relay_id: &str,
        retry: bool,
    ) -> JoinHandle<Result<crate::proto::TopicRelay, ClientError>> {
        let http = self.http();
        let shared = self.shared().clone();
        let (g, t, id) = (
            group_id.to_string(),
            topic_id.to_string(),
            relay_id.to_string(),
        );
        self.spawn(async move {
            let relay = if retry {
                http.retry_relay(&g, &id).await?
            } else {
                http.stop_relay(&g, &id).await?
            };
            if shared.store.store_topic_relay(&t, &relay)? {
                shared.emit(ClientUpdate::TopicsChanged { group_id: g });
            }
            Ok(relay)
        })
    }

    /// Usable: stops chain `chain_id` of group `group_id` and every run in it (全部停止).
    pub fn stop_chain(
        &self,
        group_id: &str,
        chain_id: &str,
    ) -> JoinHandle<Result<crate::proto::ChainStopResponse, ClientError>> {
        let http = self.http();
        let (g, id) = (group_id.to_string(), chain_id.to_string());
        self.spawn(async move { Ok(http.stop_chain(&g, &id).await?) })
    }

    // ------------------------------------------------------------------
    // Updates and diagnostics
    // ------------------------------------------------------------------

    /// The newest release when it is newer than `current_version`.
    pub fn check_update(
        &self,
        current_version: &str,
    ) -> JoinHandle<Result<Option<UpdateInfo>, ClientError>> {
        let http = self.http();
        let cur = current_version.to_string();
        self.spawn(async move { Ok(crate::updates::check(&http, &cur).await?) })
    }

    /// [`Client::check_update`] for a named release platform (tests).
    pub fn check_update_for(
        &self,
        current_version: &str,
        platform: &str,
    ) -> JoinHandle<Result<Option<UpdateInfo>, ClientError>> {
        let http = self.http();
        let cur = current_version.to_string();
        let platform = platform.to_string();
        self.spawn(async move { Ok(crate::updates::check_for(&http, &cur, &platform).await?) })
    }

    /// Downloads a release file (the DMG, or the Windows installer) to `dest` and checks
    /// its SHA-256.
    pub fn download_update(
        &self,
        info: UpdateInfo,
        dest: &Path,
        progress: ProgressFn,
    ) -> JoinHandle<Result<PathBuf, ClientError>> {
        let http = self.http();
        let dest = dest.to_path_buf();
        self.spawn(async move {
            crate::updates::download(&http, &info, &dest, &progress).await?;
            Ok(dest)
        })
    }

    /// Uploads the recent logs of the data directory (secrets removed, gzip) with
    /// `header` on top. Returns the report id.
    pub fn send_diagnostics(&self, header: &str) -> JoinHandle<Result<String, ClientError>> {
        let http = self.http();
        let dir = self.config().data_dir.clone();
        let header = header.to_string();
        self.spawn(async move { Ok(crate::diagnostics::send(&http, &dir, &header).await?) })
    }
}

fn store_member(shared: &crate::client::Shared, m: &Member) -> Result<(), ClientError> {
    shared
        .store
        .apply_directory(Some(std::slice::from_ref(m)), None, None)?;
    shared.emit(ClientUpdate::DirectoryChanged);
    Ok(())
}

async fn load_artifact(
    http: &crate::HttpClient,
    shared: &crate::client::Shared,
    id: &str,
) -> Result<ArtifactRow, ClientError> {
    let r = http.artifact(id).await?;
    shared.store.store_artifact_response(&r)?;
    shared.emit(ClientUpdate::ArtifactsChanged {
        group_id: r.artifact.group_id.clone(),
        topic_id: r.artifact.topic_id.clone(),
    });
    shared.store.artifact(id)?.ok_or(ClientError::NotFound)
}

/// `avatars/<sha256 of the URL>`: a new avatar has a new URL, so a new file.
fn avatar_path(dir: &std::path::Path, url: &str) -> PathBuf {
    use sha2::Digest;
    dir.join(crate::files::hex(&sha2::Sha256::digest(url.as_bytes())))
}
