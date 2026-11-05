// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The tenant a call was resolved to, carried out of the handler to where the
//! call's metric is recorded (audit H11).
//!
//! The handler opens its `TenantContext` scope inside the call's future, and
//! the call is recorded when that future has ended, outside the scope; so
//! the resolved tenant is written into a cell the call owns, here.

use std::sync::{Arc, OnceLock};

tokio::task_local! {
    static CALL_TENANT: Arc<OnceLock<String>>;
}

/// The cell a call's tenant is written to.
pub type Cell = Arc<OnceLock<String>>;

/// A future running with a call's tenant cell.
pub type Scoped<F> = tokio::task::futures::TaskLocalFuture<Cell, F>;

/// Runs `fut` with `cell` as the current call's tenant cell.
pub fn scope<F: std::future::Future>(cell: Cell, fut: F) -> Scoped<F> {
    CALL_TENANT.scope(cell, fut)
}

/// Records `tenant` as the current call's, the first time it is resolved;
/// outside a call, nothing.
pub fn note(tenant: &str) {
    let _ = CALL_TENANT.try_with(|cell| cell.set(tenant.to_owned()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_first_tenant_noted_in_a_call_is_kept() {
        let cell = Cell::default();
        scope(Arc::clone(&cell), async {
            note("acme");
            note("other");
        })
        .await;
        assert_eq!(cell.get().map(String::as_str), Some("acme"));
        note("outside");
    }
}
