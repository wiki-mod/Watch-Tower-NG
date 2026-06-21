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

pub use common_templates::{COMMON_TEMPLATES, common_template, default_template};
pub use email::{EmailSettings, NotificationUrlError, build_email_url};
pub use funcs::{template_title, template_to_json, template_to_lower, template_to_upper};
pub use gotify::{GotifySettings, build_gotify_url};
pub use model::{Data, NotificationEntry, StaticData, TemplateDataInput};
pub use msteams::{
    MS_TEAMS_TYPE, MsTeamsNotifier, MsTeamsNotifierInput, TeamsSettings, build_teams_url,
    new_msteams_notifier,
};
pub use notifier::{COLOR_HEX, COLOR_INT, get_delay, get_template_data, get_title};
pub use shoutrrr::get_scheme;
pub use slack::{SlackSettings, build_slack_url};
