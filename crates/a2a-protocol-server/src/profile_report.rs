// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What a handler's profiles guarantee, what an escape hatch gave up, and what
//! no process can check about itself (adversarial review of 2026-11-13: "keep
//! explicit escape hatches but report which guarantees they disable").

/// The guarantees of the profiles a handler was built with, as `build()`
/// left them: [`RequestHandler::profile_report`](crate::RequestHandler::profile_report).
///
/// `build()` refuses a profile whose requirements are not met, so every entry
/// of [`checked`](Self::checked) held when the handler was built. An entry of
/// [`disabled`](Self::disabled) is a guarantee one of the profiles would
/// otherwise give, turned off by a call the builder was given. An entry of
/// [`not_verifiable`](Self::not_verifiable) is something the profile depends
/// on that the deployment has to show by other means: a running process
/// cannot see its own backups, the network in front of it, or where its keys
/// are kept.
///
/// A handler built with no profile has an empty report: nothing was checked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ProfileReport {
    /// The profiles applied, in the order they were applied.
    pub profiles: Vec<&'static str>,
    /// What `build()` verified.
    pub checked: Vec<&'static str>,
    /// Each escape hatch the builder was given, and what it gives up.
    pub disabled: Vec<(&'static str, &'static str)>,
    /// What the profiles rely on that no check at startup can establish.
    pub not_verifiable: Vec<&'static str>,
}

impl ProfileReport {
    /// Adds `items` to `list`, each once.
    pub(crate) fn add(list: &mut Vec<&'static str>, items: &[&'static str]) {
        for item in items {
            if !list.contains(item) {
                list.push(item);
            }
        }
    }
}

impl std::fmt::Display for ProfileReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.profiles.is_empty() {
            return f.write_str("no deployment profile: nothing was checked");
        }
        writeln!(f, "profiles: {}", self.profiles.join(", "))?;
        for item in &self.checked {
            writeln!(f, "  checked: {item}")?;
        }
        for (hatch, lost) in &self.disabled {
            writeln!(f, "  disabled by {hatch}: {lost}")?;
        }
        for item in &self.not_verifiable {
            writeln!(f, "  not verifiable here: {item}")?;
        }
        Ok(())
    }
}
