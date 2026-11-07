//! Names shared by application modules through `use crate::app::prelude::*`.

pub(in crate::app) use std::cell::RefCell;
pub(in crate::app) use std::collections::{HashMap, HashSet};
pub(in crate::app) use std::fs;
pub(in crate::app) use std::ops::Range;
pub(in crate::app) use std::path::PathBuf;
pub(in crate::app) use std::sync::atomic::{AtomicBool, Ordering};
pub(in crate::app) use std::sync::mpsc::{self, Receiver, Sender};
pub(in crate::app) use std::sync::Arc;
pub(in crate::app) use std::time::Duration;

pub(in crate::app) use gpui_kit::assets::IconName as KitIcon;
pub(in crate::app) use gpui_kit::component::{
    box_shadow,
    button::{Button, ButtonCustomVariant, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{ContextMenuExt, DropdownMenu},
    notification::{Notification, NotificationType},
    scroll::{ScrollableElement, Scrollbar},
    slider::{Slider, SliderEvent, SliderState, SliderValue},
    switch::Switch,
    tooltip::Tooltip,
    v_flex, window_paddings, ActiveTheme, Disableable, Icon, IconName, Selectable, Sizable, Theme,
    ThemeMode, TitleBar, WindowExt,
};
pub(in crate::app) use gpui_kit::prelude::FluentBuilder as _;
pub(in crate::app) use gpui_kit::{
    div, ease_out_quint, hsla, img, linear_color_stop, linear_gradient, percentage, point, px,
    relative, rems, uniform_list, Animation, AnimationExt, App, AppContext, Context, Entity,
    FontWeight, Hsla, Image as GpuiImage, ImageFormat as GpuiImageFormat, InteractiveElement,
    IntoElement, Keystroke, MouseButton, ObjectFit, ParentElement, PathPromptOptions, Pixels,
    Render, ScrollHandle, ScrollWheelEvent, SharedString, StatefulInteractiveElement, Styled,
    StyledImage, Subscription, Transformation, UniformListScrollHandle, Window, WindowControlArea,
};
pub(in crate::app) use rust_i18n::t;
pub(in crate::app) use smol::Timer;
pub(in crate::app) use tokio::runtime::Runtime;

pub(in crate::app) use crate::api::{format_duration, OpenSubsonicProvider};
pub(in crate::app) use crate::assets::{AppIcon, PlayerIcon};
pub(in crate::app) use crate::audio::{
    format_playback, AudioHandle, PlaybackState, StreamFailure, TrackTransition,
    PLAYBACK_RECONNECT_ATTEMPTS,
};
pub(in crate::app) use crate::config;
pub(in crate::app) use crate::global_shortcuts::{
    GlobalShortcutEvent, GlobalShortcutManager, ValidationError as GlobalShortcutValidationError,
};
pub(in crate::app) use crate::media_session::{self, MediaCommand, MediaSession, MediaUpdate};
pub(in crate::app) use crate::models::{
    Album, AlbumSort, Artist, ArtistSort, Config, EqSettings, FavoriteKey, FavoriteKind, Favorites,
    Genre, LanguagePreference, PlayModes, PlaybackQueue, PlaybackSession, Playlist, QueueEntry,
    QueueOrigin, RepeatMode, SearchResults, ServerInfo, ServerProfile, ShortcutAction, Song,
    ThemePreference, TranscodingQuality, VolumeNormalization,
};
pub(in crate::app) use crate::msg::{error_message, Msg};
pub(in crate::app) use crate::notify;
pub(in crate::app) use crate::secrets;
pub(in crate::app) use crate::single_instance;
pub(in crate::app) use crate::tray::{self, TrayCommand};
pub(in crate::app) use crate::vinyl::VinylCover;

pub(in crate::app) use crate::app::covers::state::CoverRequest;
pub(in crate::app) use crate::app::covers::texture_state::CoverTextures;
pub(in crate::app) use crate::app::desktop::platform_window::*;
pub(in crate::app) use crate::app::desktop::state::DesktopState;
pub(in crate::app) use crate::app::home::albums::HomeAlbumSection;
pub(in crate::app) use crate::app::import::flow::ImportStage;
pub(in crate::app) use crate::app::import::orchestration::ImportUi;
pub(in crate::app) use crate::app::library::paged_collection::PageRequest;
pub(in crate::app) use crate::app::library::paged_collection::PageRequests;
pub(in crate::app) use crate::app::library::requests::LibraryRequest;
pub(in crate::app) use crate::app::library::requests::LibraryRequests;
pub(in crate::app) use crate::app::playback::preload_state::PreloadState;
pub(in crate::app) use crate::app::playback::reconnect::PlaybackReconnect;
pub(in crate::app) use crate::app::playback::session_state::SessionState;
pub(in crate::app) use crate::app::player::mini_window_state::MiniWindowState;
pub(in crate::app) use crate::app::playlists::dialog::PlaylistUi;
pub(in crate::app) use crate::app::playlists::edit_state::PlaylistEditOrigin;
pub(in crate::app) use crate::app::recommendations::auto_continue_state::AutoContinueRequest;
pub(in crate::app) use crate::app::recommendations::daily_mix_state::DailyMixRequest;
pub(in crate::app) use crate::app::recommendations::radio_state::RadioRequest;
pub(in crate::app) use crate::app::search::state::SearchFilter;
pub(in crate::app) use crate::app::server::removal_state::ServerRemovalState;
pub(in crate::app) use crate::app::settings::state::{SettingsSection, ShortcutRecording};
pub(in crate::app) use crate::app::shell::state::AppState;
pub(in crate::app) use crate::app::shell::view_state::{QueueDrawerTab, View};
pub(in crate::app) use crate::app::shortcuts::repeat::ShortcutRepeatGuard;
pub(in crate::app) use crate::app::songs::scope::SongScope;
pub(in crate::app) use crate::app::sound::equalizer_state::EqualizerState;
pub(in crate::app) use crate::app::sound::volume_state::VolumeState;
pub(in crate::app) use crate::app::style::buttons::{
    capsule_button, round_play_button, soft_toggle_style,
};
pub(in crate::app) use crate::app::style::surface::{cover_shadow, raised_surface};
pub(in crate::app) use crate::app::style::text::{section_title, serif_heading};
pub(in crate::app) use crate::app::style::tokens::{self, palette};
pub(in crate::app) use crate::app::widgets::scrolling_text_state::TextWidthCache;
pub(in crate::app) use crate::app::NavidromeApp;
