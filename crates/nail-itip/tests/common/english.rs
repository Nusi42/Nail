//! `itip.ftl` rendered in English for the text tests (unit tests include
//! this file with `#[path]`, the integration tests as a module).

use fluent_bundle::{FluentArgs, FluentBundle, FluentResource, FluentValue};
use nail_api::localized::{LocalizedMessage, MessageArg};

/// Renders `itip.ftl` like nail-config's catalog (no isolation marks).
pub(crate) struct English(FluentBundle<FluentResource>);

impl English {
    pub(crate) fn new() -> Self {
        let mut bundle = FluentBundle::new(vec!["en".parse().unwrap()]);
        bundle.set_use_isolating(false);
        let text = include_str!("../../../../locales/en/itip.ftl").to_owned();
        bundle
            .add_resource(FluentResource::try_new(text).expect("itip.ftl parses"))
            .unwrap();
        Self(bundle)
    }

    /// The message's English text (panics on a missing id or a formatting
    /// error).
    pub(crate) fn text(&self, message: &LocalizedMessage) -> String {
        let pattern = self
            .0
            .get_message(&message.id)
            .and_then(|m| m.value())
            .unwrap_or_else(|| panic!("missing {}", message.id));
        let mut args = FluentArgs::new();
        for (name, value) in &message.args {
            match value {
                MessageArg::Number(n) => args.set(name.as_str(), FluentValue::from(*n)),
                MessageArg::Text(t) => args.set(name.as_str(), FluentValue::from(t.as_str())),
            }
        }
        let mut errors = Vec::new();
        let text = self.0.format_pattern(pattern, Some(&args), &mut errors);
        assert!(errors.is_empty(), "{}: {errors:?}", message.id);
        text.into_owned()
    }
}
