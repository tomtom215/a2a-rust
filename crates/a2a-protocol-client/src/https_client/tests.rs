// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The settings a pinned client is built from.

use std::time::Duration;

use super::pinned_settings;
use crate::pool::Settings;

/// A pinned client connects within the timeout it was given, not the pool's
/// default, and takes every other setting from the pool's defaults.
#[test]
fn a_pinned_client_takes_the_connect_timeout_it_was_given() {
    let defaults = Settings::default();
    let given = defaults.connect_timeout + Duration::from_secs(7);
    let settings = pinned_settings(given);
    assert_eq!(settings.connect_timeout, given);
    assert_eq!(settings.idle_timeout, defaults.idle_timeout);
    assert_eq!(settings.max_idle_per_host, defaults.max_idle_per_host);
}
