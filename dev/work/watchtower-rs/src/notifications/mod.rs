#![forbid(unsafe_code)]

pub mod common_templates;
pub mod email;
pub mod funcs;
pub mod gotify;
pub mod json;
pub mod model;
pub mod msteams;
pub mod notifier;
pub mod preview;
pub mod runtime;
pub mod shoutrrr;
pub mod slack;

pub use common_templates::{common_template, default_template, COMMON_TEMPLATES};
pub use email::{build_email_url, EmailSettings, NotificationUrlError};
pub use funcs::{template_title, template_to_json, template_to_lower, template_to_upper};
pub use gotify::{build_gotify_url, GotifySettings};
pub use model::{Data, NotificationEntry, StaticData, TemplateDataInput};
pub use msteams::{
    build_teams_url, new_msteams_notifier, MsTeamsNotifier, MsTeamsNotifierInput, MS_TEAMS_TYPE,
    TeamsSettings,
};
pub use notifier::{get_delay, get_template_data, get_title, COLOR_HEX, COLOR_INT};
pub use shoutrrr::get_scheme;
pub use slack::{build_slack_url, SlackSettings};
