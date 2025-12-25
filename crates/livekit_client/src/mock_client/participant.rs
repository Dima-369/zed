use crate::{
    AudioStream, LocalAudioTrack, LocalTrackPublication, LocalVideoTrack, Participant, RemoteTrack,
    RemoteTrackPublication, TrackSid,
    mock_client::{RemoteAudioTrack, RemoteVideoTrack, Room, WeakRoom},
    shared_types::ParticipantIdentity,
};
use anyhow::Result;
use collections::HashMap;
use gpui::{
    AsyncApp, DevicePixels, ScreenCaptureSource, ScreenCaptureStream, SourceMetadata, size,
};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct LocalParticipant {
    pub(crate) identity: ParticipantIdentity,
    pub(crate) room: Room,
}

#[derive(Clone, Debug)]
pub struct RemoteParticipant {
    pub(crate) identity: ParticipantIdentity,
    pub(crate) room: WeakRoom,
}

impl Participant {
    pub fn identity(&self) -> ParticipantIdentity {
        match self {
            Participant::Local(participant) => participant.identity.clone(),
            Participant::Remote(participant) => participant.identity.clone(),
        }
    }
}

impl LocalParticipant {
    pub fn new() -> Self {
        Self {
            identity: ParticipantIdentity("mock_local_participant".to_string()),
            room: Room::new(),
        }
    }

    pub async fn unpublish_track(&self, track: TrackSid, _cx: &AsyncApp) -> Result<()> {
        self.room
            .test_server()
            .unpublish_track(self.room.token(), &track)
            .await
    }

    #[allow(dead_code)] // Only used when webrtc feature is enabled
    pub(crate) async fn publish_microphone_track(
        &self,
        _cx: &AsyncApp,
    ) -> Result<(LocalTrackPublication, AudioStream)> {
        let this = self.clone();
        let server = this.room.test_server();
        let sid = server
            .publish_audio_track(this.room.token(), &LocalAudioTrack {})
            .await?;

        Ok((
            LocalTrackPublication {
                room: self.room.downgrade(),
                sid,
            },
            AudioStream {},
        ))
    }

    pub async fn publish_screenshare_track(
        &self,
        _source: &dyn ScreenCaptureSource,
        _cx: &mut AsyncApp,
    ) -> Result<(LocalTrackPublication, Box<dyn ScreenCaptureStream>)> {
        let this = self.clone();
        let server = this.room.test_server();
        let sid = server
            .publish_video_track(this.room.token(), LocalVideoTrack {})
            .await?;
        Ok((
            LocalTrackPublication {
                room: self.room.downgrade(),
                sid,
            },
            Box::new(TestScreenCaptureStream {}),
        ))
    }
}

impl RemoteParticipant {
    pub fn track_publications(&self) -> HashMap<TrackSid, RemoteTrackPublication> {
        if let Some(room) = self.room.upgrade() {
            let server = room.test_server();
            let audio = server
                .audio_tracks(room.token())
                .unwrap()
                .into_iter()
                .filter(|track| track.publisher_id() == self.identity)
                .map(|track| {
                    let remote_track = RemoteAudioTrack {
                        server_track: Arc::new(track.clone()),
                        room: self.room.clone(),
                    };
                    (
                        track.sid(),
                        RemoteTrackPublication {
                            sid: track.sid(),
                            room: self.room.clone(),
                            track: RemoteTrack::Audio(remote_track),
                        },
                    )
                });
            let video = server
                .video_tracks(room.token())
                .unwrap()
                .into_iter()
                .filter(|track| track.publisher_id() == self.identity)
                .map(|track| {
                    let remote_track = RemoteVideoTrack {
                        server_track: Arc::new(track.clone()),
                        _room: self.room.clone(),
                    };
                    (
                        track.sid(),
                        RemoteTrackPublication {
                            sid: track.sid(),
                            room: self.room.clone(),
                            track: RemoteTrack::Video(remote_track),
                        },
                    )
                });
            audio.chain(video).collect()
        } else {
            HashMap::default()
        }
    }

    pub fn identity(&self) -> ParticipantIdentity {
        self.identity.clone()
    }
}

struct TestScreenCaptureStream;

impl ScreenCaptureStream for TestScreenCaptureStream {
    fn metadata(&self) -> Result<SourceMetadata> {
        Ok(SourceMetadata {
            id: 0,
            is_main: None,
            label: None,
            resolution: size(DevicePixels(1), DevicePixels(1)),
        })
    }
}
