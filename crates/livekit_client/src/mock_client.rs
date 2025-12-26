use crate::shared_types;

pub(crate) mod participant;
pub(crate) mod publication;
pub(crate) mod track;

pub type RemoteVideoTrack = track::RemoteVideoTrack;
pub type RemoteAudioTrack = track::RemoteAudioTrack;
pub type RemoteTrackPublication = publication::RemoteTrackPublication;
pub type RemoteParticipant = participant::RemoteParticipant;

pub type LocalVideoTrack = track::LocalVideoTrack;
pub type LocalAudioTrack = track::LocalAudioTrack;
pub type LocalTrackPublication = publication::LocalTrackPublication;
pub type LocalParticipant = participant::LocalParticipant;

pub use shared_types::{ConnectionState, ParticipantIdentity, TrackSid};

use anyhow;
use collections::HashSet;
use futures::channel::mpsc;
use gpui::{App, AsyncApp};
use std::sync::{Arc, Mutex};

// Mock Room type for when webrtc is disabled
#[derive(Clone, Debug)]
pub struct Room(pub Arc<Mutex<RoomState>>);

#[derive(Debug, Default)]
pub struct RoomState {
    pub paused_audio_tracks: HashSet<TrackSid>,
}

pub type WeakRoom = std::sync::Weak<Room>;

impl Room {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(RoomState::default())))
    }

    pub fn downgrade(&self) -> WeakRoom {
        Arc::downgrade(&Arc::new(self.clone()))
    }

    pub fn test_server(&self) -> MockTestServer {
        MockTestServer
    }

    pub fn token(&self) -> &str {
        "mock_token"
    }

    // Mock methods needed by call crate
    pub async fn sid(&self) -> String {
        "mock_room_sid".to_string()
    }

    pub fn name(&self) -> String {
        "mock_room_name".to_string()
    }

    pub fn connection_state(&self) -> ConnectionState {
        ConnectionState::Connected
    }

    pub fn remote_participants(
        &self,
    ) -> collections::HashMap<ParticipantIdentity, participant::RemoteParticipant> {
        collections::HashMap::default()
    }

    pub fn play_remote_audio_track(
        &self,
        _track: &track::RemoteAudioTrack,
        _cx: &App,
    ) -> anyhow::Result<AudioStream> {
        Err(anyhow::anyhow!("WebRTC disabled - cannot play audio track"))
    }

    pub async fn publish_local_microphone_track(
        &self,
        _user_name: String,
        _is_staff: bool,
        _cx: &mut AsyncApp,
    ) -> anyhow::Result<(publication::LocalTrackPublication, AudioStream)> {
        Err(anyhow::anyhow!(
            "WebRTC disabled - cannot publish microphone track"
        ))
    }

    pub async fn unpublish_local_track(
        &self,
        _track_sid: TrackSid,
        _cx: &mut AsyncApp,
    ) -> anyhow::Result<publication::LocalTrackPublication> {
        Err(anyhow::anyhow!("WebRTC disabled - cannot unpublish track"))
    }

    pub fn local_participant(&self) -> participant::LocalParticipant {
        participant::LocalParticipant::new()
    }

    pub async fn connect(
        _server_url: String,
        _token: String,
        _cx: &mut AsyncApp,
    ) -> anyhow::Result<(Self, mpsc::UnboundedReceiver<crate::RoomEvent>)> {
        Err(anyhow::anyhow!("WebRTC disabled - cannot connect to room"))
    }
}

pub struct MockTestServer;

impl MockTestServer {
    pub async fn unpublish_track(&self, _token: &str, _track: &TrackSid) -> anyhow::Result<()> {
        Ok(())
    }

    pub async fn publish_audio_track(
        &self,
        _token: &str,
        _track: &track::LocalAudioTrack,
    ) -> anyhow::Result<TrackSid> {
        Ok(TrackSid("mock_audio_track".to_string()))
    }

    pub async fn publish_video_track(
        &self,
        _token: &str,
        _track: track::LocalVideoTrack,
    ) -> anyhow::Result<TrackSid> {
        Ok(TrackSid("mock_video_track".to_string()))
    }

    pub fn audio_tracks(&self, _token: &str) -> anyhow::Result<Vec<track::TestServerAudioTrack>> {
        Ok(vec![])
    }

    pub fn video_tracks(&self, _token: &str) -> anyhow::Result<Vec<track::TestServerVideoTrack>> {
        Ok(vec![])
    }

    pub async fn set_track_muted(
        &self,
        _token: &str,
        _track: &TrackSid,
        _mute: bool,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    pub fn is_track_muted(&self, _token: &str, _track: &TrackSid) -> anyhow::Result<bool> {
        Ok(false)
    }
}

// Re-export test server track types
pub use track::{TestServerAudioTrack, TestServerVideoTrack};

pub struct AudioStream {}

#[cfg(not(target_os = "macos"))]
pub type RemoteVideoFrame = std::sync::Arc<gpui::RenderImage>;

#[cfg(target_os = "macos")]
#[derive(Clone)]
pub(crate) struct RemoteVideoFrame {}
#[cfg(target_os = "macos")]
impl Into<gpui::SurfaceSource> for RemoteVideoFrame {
    fn into(self) -> gpui::SurfaceSource {
        unimplemented!()
    }
}
pub(crate) fn play_remote_video_track(
    _track: &crate::RemoteVideoTrack,
) -> impl futures::Stream<Item = RemoteVideoFrame> + use<> {
    futures::stream::pending()
}
