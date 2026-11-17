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
    /// What the deployment declared about the controls outside this process
    /// ([`declare_external_control`](crate::RequestHandlerBuilder::declare_external_control)),
    /// as declared: nothing here was verified.
    pub declared: Vec<(ExternalControl, String)>,
}

/// A control outside this process that a profile relies on and cannot see,
/// declared with
/// [`RequestHandlerBuilder::declare_external_control`](crate::RequestHandlerBuilder::declare_external_control).
///
/// A declaration is a statement for the record, not a check: it is carried
/// into [`ProfileReport::declared`] so that what the deployment said it does
/// is logged beside what the handler checked, and can be compared with the
/// deployment's evidence. [`Profile::RegulatedData`](crate::profile::Profile::RegulatedData)
/// refuses to build until each control it relies on has been declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ExternalControl {
    /// Which proxies may set forwarded headers (`Forwarded`, `X-Forwarded-For`)
    /// that a rate limiter or credential throttle reads, or that none do.
    TrustedProxies,
    /// Where the key-encryption keys of sealed stores and audit privacy are
    /// held, who can use them, and how they are rotated.
    KeyCustody,
    /// That the databases are backed up, and a restore has been rehearsed.
    Backups,
    /// The retention schedule, and the jobs that apply it (`purge_expired`,
    /// `AuditLog::purge`, `StepJournal::sweep`).
    Retention,
    /// That task content is encrypted at rest by the storage itself (the
    /// database or the disk), in place of `EncryptedTaskStore` (feature
    /// `encryption`).
    EncryptionAtRest,
}

impl ExternalControl {
    /// A short name for the control, as reports print it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::TrustedProxies => "trusted proxies",
            Self::KeyCustody => "key custody",
            Self::Backups => "backups and restore",
            Self::Retention => "retention",
            Self::EncryptionAtRest => "encryption at rest",
        }
    }
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
        for (control, how) in &self.declared {
            writeln!(f, "  declared, not verified: {}: {how}", control.name())?;
        }
        Ok(())
    }
}
