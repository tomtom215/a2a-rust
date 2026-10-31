// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! An error written with its causes.
//!
//! hyper's own text, `client error (Connect)` or `client error (SendRequest)`,
//! does not say what went wrong; the cause (`invalid peer certificate:
//! UnknownIssuer`, `Connection refused`) is in the error's `source` chain.
//! Push delivery, JWKS and OIDC discovery and the model client report through
//! this, so an operator reads the reason where the failure is logged (audit
//! B-05 fixed the same fault in the client crate).

/// `error` and its causes, joined by `: `.
pub fn with_causes(error: &(dyn std::error::Error + 'static)) -> String {
    let mut text = error.to_string();
    let mut cause = error.source();
    while let Some(next) = cause {
        text.push_str(": ");
        text.push_str(&next.to_string());
        cause = next.source();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Outer(std::io::Error);
    impl std::fmt::Display for Outer {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("client error (SendRequest)")
        }
    }
    impl std::error::Error for Outer {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.0)
        }
    }

    /// A transport error's text names its causes: hyper's own, `client error
    /// (SendRequest)`, says nothing of what went wrong.
    #[test]
    fn an_error_is_written_with_its_causes() {
        let error = Outer(std::io::Error::other(
            "connection closed before message completed",
        ));
        assert_eq!(
            with_causes(&error),
            "client error (SendRequest): connection closed before message completed"
        );
    }

    #[test]
    fn an_error_without_a_cause_is_written_as_it_is() {
        let error = std::io::Error::other("alone");
        assert_eq!(with_causes(&error), "alone");
    }
}
