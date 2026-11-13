mod plugins;
pub use plugins::EnhancedAudioPlugin;

use std::{collections::HashMap, io::Cursor, sync::Arc, time::Duration};

use bevy_app::{App, Plugin};
use bevy_asset::{AssetPath, AssetServer, Assets, Handle, LoadState};
use bevy_audio::{
    AudioPlayer, AudioPlugin, AudioSink, AudioSinkPlayback, AudioSource, GlobalVolume,
    PlaybackSettings, Volume,
};
use bevy_ecs::prelude::*;
use bevy_time::{Real, Time};
use inslag_state_tree::{
    StateTreeActivityLifecycle, StateTreeActivityOutcomeWriter, StateTreeActivityToken,
    StateTreeExt, StateTreeInstance, StateTreePlugin, StateTreeSet,
};

use crate::{
    AudioDiagnostic, AudioMixer, AudioSettings, EnhancedAudioSet,
    state_tree::{BusGain, Play, Playback, Sound},
};

type Token = StateTreeActivityToken<Playback>;

struct Request {
    owner: Entity,
    sound: Sound,
    token: Option<Token>,
    looping: bool,
    queued: Duration,
    sequence: u64,
}

enum Phase {
    Loading,
    Starting { entity: Entity, since: Duration },
    Playing { entity: Entity },
}

struct Voice {
    request: Request,
    handle: Handle<AudioSource>,
    phase: Phase,
}

impl Voice {
    fn entity(&self) -> Option<Entity> {
        match self.phase {
            Phase::Loading => None,
            Phase::Starting { entity, .. } | Phase::Playing { entity } => Some(entity),
        }
    }
}

struct CachedSource {
    handle: Handle<AudioSource>,
    touched: u64,
    validation: Option<Validation>,
}

struct Validation {
    bytes: Arc<[u8]>,
    result: Result<(), String>,
}

#[derive(Resource, Default)]
struct Runtime {
    pending: Vec<Request>,
    voices: Vec<Voice>,
    cache: HashMap<AssetPath<'static>, CachedSource>,
    sequence: u64,
    cache_clock: u64,
}

impl Runtime {
    fn request(
        &mut self,
        owner: Entity,
        sound: Sound,
        token: Option<Token>,
        looping: bool,
        now: Duration,
    ) -> Request {
        let sequence = self.sequence;
        self.sequence = self.sequence.wrapping_add(1);
        Request {
            owner,
            sound,
            token,
            looping,
            queued: now,
            sequence,
        }
    }

    fn enqueue(&mut self, request: Request, limit: usize) {
        if limit == 0 {
            return;
        }
        if self.pending.len() < limit {
            self.pending.push(request);
        } else if let Some((index, weakest)) =
            self.pending.iter().enumerate().min_by_key(|(_, pending)| {
                (pending.sound.priority, std::cmp::Reverse(pending.sequence))
            })
            && request.sound.priority > weakest.sound.priority
        {
            self.pending[index] = request;
        }
    }

    fn source(
        &mut self,
        src: &AssetPath<'static>,
        server: &AssetServer,
        capacity: usize,
    ) -> Handle<AudioSource> {
        self.trim_cache(capacity);
        self.cache_clock = self.cache_clock.wrapping_add(1);
        if let Some(entry) = self.cache.get_mut(src) {
            entry.touched = self.cache_clock;
            return entry.handle.clone();
        }
        let handle = server.load(src.clone());
        if capacity > 0 {
            self.trim_cache(capacity - 1);
            self.cache.insert(
                src.clone(),
                CachedSource {
                    handle: handle.clone(),
                    touched: self.cache_clock,
                    validation: None,
                },
            );
        }
        handle
    }

    fn trim_cache(&mut self, capacity: usize) {
        while self.cache.len() > capacity {
            let oldest = self
                .cache
                .iter()
                .min_by(|(a_path, a), (b_path, b)| {
                    a.touched
                        .cmp(&b.touched)
                        .then_with(|| a_path.to_string().cmp(&b_path.to_string()))
                })
                .map(|(path, _)| path.clone())
                .unwrap();
            self.cache.remove(&oldest);
        }
    }

    fn validate(&mut self, src: &AssetPath<'static>, source: &AudioSource) -> Result<(), String> {
        if let Some(validation) = self
            .cache
            .get(src)
            .and_then(|entry| entry.validation.as_ref())
            && Arc::ptr_eq(&validation.bytes, &source.bytes)
        {
            return validation.result.clone();
        }
        // Bevy's AudioSource::decoder unwraps decoding errors. Validate before
        // handing the source to its playback system, including after reloads.
        let result = rodio::Decoder::builder()
            .with_byte_len(source.bytes.len() as u64)
            .with_data(Cursor::new(source.clone()))
            .build()
            .map(|_| ())
            .map_err(|error| format!("cannot decode audio: {error}"));
        if let Some(entry) = self.cache.get_mut(src) {
            entry.validation = Some(Validation {
                bytes: source.bytes.clone(),
                result: result.clone(),
            });
        }
        result
    }
}

fn collect_effects(
    mut plays: MessageReader<Play>,
    mut gains: MessageReader<BusGain>,
    time: Res<Time<Real>>,
    settings: Res<AudioSettings>,
    mut runtime: ResMut<Runtime>,
    mut mixer: ResMut<AudioMixer>,
    mut diagnostics: MessageWriter<AudioDiagnostic>,
) {
    for message in gains.read() {
        if let Err(error) = mixer.set_authored(&message.bus, message.gain) {
            tracing::warn!(owner = ?message.owner, bus = %message.bus, %error, "audio.set_bus_gain failed");
            let diagnostic = AudioDiagnostic {
                owner: message.owner,
                src: String::new(),
                reason: error.to_string(),
            };
            diagnostics.write(diagnostic);
        }
    }
    for message in plays.read() {
        let request = runtime.request(
            message.owner,
            message.sound.clone(),
            None,
            false,
            time.elapsed(),
        );
        runtime.enqueue(request, settings.max_pending);
    }
}

#[derive(bevy_ecs::system::SystemParam)]
struct ActivityContext<'w, 's> {
    instances: Query<'w, 's, &'static StateTreeInstance>,
    outcomes: StateTreeActivityOutcomeWriter<'w, 's, Playback>,
    commands: Commands<'w, 's>,
}

fn collect_activities(
    mut lifecycle: MessageReader<StateTreeActivityLifecycle<Playback>>,
    time: Res<Time<Real>>,
    settings: Res<AudioSettings>,
    server: Res<AssetServer>,
    mut runtime: ResMut<Runtime>,
    mut context: ActivityContext,
) {
    for message in lifecycle.read() {
        match message {
            StateTreeActivityLifecycle::Started { token, activity } => {
                if !context
                    .instances
                    .get(token.entity())
                    .is_ok_and(|instance| instance.is_activity_active(*token))
                {
                    continue;
                }
                if runtime
                    .voices
                    .iter()
                    .filter(|voice| voice.request.token.is_some())
                    .count()
                    >= settings.max_activities
                {
                    context
                        .outcomes
                        .fail(*token, "audio activity budget exhausted".into());
                    continue;
                }
                let request = runtime.request(
                    token.entity(),
                    activity.sound.clone(),
                    Some(*token),
                    activity.looping,
                    time.elapsed(),
                );
                let handle = runtime.source(&request.sound.src, &server, settings.cache_capacity);
                runtime.voices.push(Voice {
                    request,
                    handle,
                    phase: Phase::Loading,
                });
            }
            StateTreeActivityLifecycle::Cancelled { token } => {
                runtime.voices.retain(|voice| {
                    if voice.request.token != Some(*token) {
                        return true;
                    }
                    if let Some(entity) = voice.entity() {
                        context.commands.entity(entity).try_despawn();
                    }
                    false
                });
            }
        }
    }
}

#[derive(bevy_ecs::system::SystemParam)]
struct PlaybackContext<'w, 's> {
    commands: Commands<'w, 's>,
    instances: Query<'w, 's, &'static StateTreeInstance>,
    players: Query<
        'w,
        's,
        (
            &'static mut PlaybackSettings,
            Option<&'static mut AudioSink>,
        ),
    >,
    outcomes: StateTreeActivityOutcomeWriter<'w, 's, Playback>,
    diagnostics: MessageWriter<'w, AudioDiagnostic>,
    global: Option<Res<'w, GlobalVolume>>,
}

#[derive(bevy_ecs::system::SystemParam)]
struct PlaybackAssets<'w> {
    server: Res<'w, AssetServer>,
    sources: Res<'w, Assets<AudioSource>>,
}

fn advance(
    time: Res<Time<Real>>,
    settings: Res<AudioSettings>,
    mixer: Res<AudioMixer>,
    assets: PlaybackAssets,
    mut runtime: ResMut<Runtime>,
    mut context: PlaybackContext,
) {
    let PlaybackAssets { server, sources } = assets;
    let now = time.elapsed();
    runtime.trim_cache(settings.cache_capacity);
    // Stable priority ordering keeps admission independent of a burst's order
    // while retaining authored order for equal-priority requests.
    runtime
        .pending
        .sort_by_key(|request| (std::cmp::Reverse(request.sound.priority), request.sequence));
    let pending = std::mem::take(&mut runtime.pending);
    for request in pending {
        if now.saturating_sub(request.queued) > settings.one_shot_max_age {
            continue;
        }
        let loading = runtime
            .voices
            .iter()
            .filter(|voice| voice.request.token.is_none() && matches!(voice.phase, Phase::Loading))
            .count();
        if loading >= settings.max_pending {
            let weakest = runtime
                .voices
                .iter()
                .enumerate()
                .filter(|(_, voice)| {
                    voice.request.token.is_none() && matches!(voice.phase, Phase::Loading)
                })
                .min_by_key(|(_, voice)| {
                    (
                        voice.request.sound.priority,
                        std::cmp::Reverse(voice.request.sequence),
                    )
                });
            if let Some((index, weakest)) = weakest
                && weakest.request.sound.priority < request.sound.priority
            {
                runtime.voices.remove(index);
            } else {
                continue;
            }
        }
        let handle = runtime.source(&request.sound.src, &server, settings.cache_capacity);
        runtime.voices.push(Voice {
            request,
            handle,
            phase: Phase::Loading,
        });
    }

    let mut voices = std::mem::take(&mut runtime.voices);
    let mut ready = Vec::new();
    let mut retained = Vec::new();
    for voice in voices.drain(..) {
        if let Some(token) = voice.request.token
            && !context
                .instances
                .get(token.entity())
                .is_ok_and(|instance| instance.is_activity_active(token))
        {
            stop(&voice, &mut context);
            continue;
        }
        if !voice.request.sound.speed.is_finite() || voice.request.sound.speed <= 0. {
            fail(
                &voice,
                "audio backend requires a finite positive playback speed".into(),
                &mut context,
            );
            continue;
        }
        let gain = match mixer.gain(&voice.request.sound.bus) {
            Ok(gain) => gain * voice.request.sound.gain,
            Err(error) => {
                fail(&voice, error.to_string(), &mut context);
                continue;
            }
        };
        if !gain.is_finite() {
            fail(
                &voice,
                "audio backend requires a finite gain".into(),
                &mut context,
            );
            continue;
        }
        match voice.phase {
            Phase::Loading => {
                if voice.request.token.is_none()
                    && (gain == 0.0
                        || now.saturating_sub(voice.request.queued) > settings.one_shot_max_age)
                {
                    continue;
                }
                match server.get_load_state(voice.handle.id()) {
                    Some(LoadState::Failed(error)) => {
                        fail(&voice, error.to_string(), &mut context);
                    }
                    Some(LoadState::Loaded) => {
                        let validation = sources
                            .get(&voice.handle)
                            .ok_or_else(|| "loaded audio source is unavailable".into())
                            .and_then(|source| runtime.validate(&voice.request.sound.src, source));
                        match validation {
                            Ok(()) => ready.push(voice),
                            Err(error) => fail(&voice, error, &mut context),
                        }
                    }
                    _ if now.saturating_sub(voice.request.queued) >= settings.load_timeout => {
                        fail(&voice, "audio loading timed out".into(), &mut context)
                    }
                    _ => retained.push(voice),
                }
            }
            Phase::Starting { entity, .. } | Phase::Playing { entity } => {
                if matches!(voice.phase, Phase::Starting { .. })
                    && let Some(source) = sources.get(&voice.handle)
                    && let Err(error) = runtime.validate(&voice.request.sound.src, source)
                {
                    fail(&voice, error, &mut context);
                    continue;
                }
                let status = match context.players.get_mut(entity) {
                    Ok((_, Some(sink))) => Ok(Some(sink.empty())),
                    Ok((_, None)) => Ok(None),
                    Err(_) => Err("audio playback entity disappeared".into()),
                };
                match status {
                    Err(error) => fail(&voice, error, &mut context),
                    Ok(Some(true)) => {
                        if let Some(token) = voice.request.token {
                            context.outcomes.complete(token, ());
                        }
                        stop(&voice, &mut context);
                    }
                    Ok(Some(false)) => {
                        sync_volume(entity, gain, &mut context);
                        retained.push(Voice {
                            phase: Phase::Playing { entity },
                            ..voice
                        });
                    }
                    Ok(None) => {
                        if matches!(voice.phase, Phase::Playing { .. }) {
                            fail(&voice, "audio sink disappeared".into(), &mut context);
                            continue;
                        }
                        let since = match voice.phase {
                            Phase::Starting { since, .. } => since,
                            _ => now,
                        };
                        if now.saturating_sub(since) >= settings.sink_timeout {
                            fail(
                                &voice,
                                "audio output did not start (device unavailable)".into(),
                                &mut context,
                            );
                        } else {
                            sync_volume(entity, gain, &mut context);
                            retained.push(voice);
                        }
                    }
                }
            }
        }
    }

    ready.sort_by_key(|voice| {
        (
            std::cmp::Reverse(voice.request.sound.priority),
            voice.request.sequence,
        )
    });
    for mut voice in ready {
        if let Some(group) = &voice.request.sound.group {
            let matching: Vec<_> = retained
                .iter()
                .filter(|old| {
                    old.entity().is_some()
                        && old.request.owner == voice.request.owner
                        && old.request.sound.group.as_ref() == Some(group)
                })
                .collect();
            let limit = matching
                .iter()
                .fold(voice.request.sound.limit, |limit, old| {
                    limit.min(old.request.sound.limit)
                });
            if matching.len() >= limit {
                if voice.request.token.is_some() {
                    fail(
                        &voice,
                        "audio concurrency limit reached".into(),
                        &mut context,
                    );
                }
                continue;
            }
        }
        if voice.request.token.is_none() {
            let active = retained
                .iter()
                .filter(|old| old.request.token.is_none() && old.entity().is_some())
                .count();
            if active >= settings.max_one_shots {
                let weakest = retained
                    .iter()
                    .enumerate()
                    .filter(|(_, old)| old.request.token.is_none() && old.entity().is_some())
                    .min_by_key(|(_, old)| (old.request.sound.priority, old.request.sequence));
                if let Some((index, old)) = weakest
                    && old.request.sound.priority < voice.request.sound.priority
                {
                    let old = retained.remove(index);
                    stop(&old, &mut context);
                } else {
                    continue;
                }
            }
        }
        let gain = mixer.gain(&voice.request.sound.bus).unwrap() * voice.request.sound.gain;
        let playback = if voice.request.looping {
            PlaybackSettings::LOOP
        } else {
            PlaybackSettings::ONCE
        }
        .with_volume(Volume::Linear(gain))
        .with_speed(voice.request.sound.speed);
        let entity = context
            .commands
            .spawn((AudioPlayer::new(voice.handle.clone()), playback))
            .id();
        voice.phase = Phase::Starting { entity, since: now };
        retained.push(voice);
    }
    runtime.voices = retained;
}

fn fail(voice: &Voice, reason: String, context: &mut PlaybackContext) {
    if let Some(token) = voice.request.token {
        context.outcomes.fail(token, reason.clone());
    }
    tracing::warn!(src = %voice.request.sound.src, %reason, "audio playback failed");
    let diagnostic = AudioDiagnostic {
        owner: voice.request.owner,
        src: voice.request.sound.src.to_string(),
        reason,
    };
    context.diagnostics.write(diagnostic);
    stop(voice, context);
}

fn stop(voice: &Voice, context: &mut PlaybackContext) {
    if let Some(entity) = voice.entity() {
        context.commands.entity(entity).try_despawn();
    }
}

fn sync_volume(entity: Entity, gain: f32, context: &mut PlaybackContext) {
    if let Ok((mut settings, sink)) = context.players.get_mut(entity) {
        let volume = Volume::Linear(gain);
        if settings.volume != volume {
            settings.volume = volume;
        }
        let output_volume = context
            .global
            .as_ref()
            .map_or(volume, |global| volume * global.volume);
        if let Some(mut sink) = sink
            && sink.volume() != output_volume
        {
            sink.set_volume(output_volume);
        }
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
