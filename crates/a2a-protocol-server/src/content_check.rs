// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`ContentCheck`], re-exported as `request_context::ContentCheck`.

/// What the server found when it checked the message's provenance
/// signature (`a2a_protocol_types::provenance`, ADR 0027).
///
/// A message whose signature does not verify never reaches the executor:
/// the send is refused. Anyone able to alter a message can also remove its
/// signature, so [`Unsigned`](Self::Unsigned) content is unverified, whoever
/// sent it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ContentCheck {
    /// No keys to check against: the handler has none set
    /// (`RequestHandlerBuilder::with_content_keys`), or the executor is
    /// driven directly. Signed or not, nothing was verified.
    #[default]
    NotChecked,
    /// The message carries no signature.
    Unsigned,
    /// The signature verified; this is the provenance it vouches for.
    Verified(a2a_protocol_types::provenance::Provenance),
}
