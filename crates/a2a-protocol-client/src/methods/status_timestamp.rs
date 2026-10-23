// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `ListTasksRequest.status_timestamp_after` in the form spec §5.6.1 puts
//! every timestamp on the wire (idx 180/184 of the conformance ledger).

use crate::error::{ClientError, ClientResult};

/// `ts` as spec §5.6.1 writes a timestamp: `YYYY-MM-DDTHH:MM:SS[.fff]Z`. A
/// value already in that form is kept as given, so its precision is; any
/// other the gRPC conversion reads (an offset, a lower-case `t` or `z`) is
/// rewritten to UTC at millisecond precision.
pub fn utc(ts: &str) -> ClientResult<String> {
    let millis = a2a_protocol_types::parse_iso8601_to_unix_millis(ts).ok_or_else(|| {
        ClientError::Protocol(a2a_protocol_types::A2aError::invalid_params(format!(
            "ListTasks not sent: statusTimestampAfter {ts:?} is not an ISO 8601 \
             date-time such as \"2023-10-27T10:00:00Z\" (spec §5.6.1)"
        )))
    })?;
    let canonical = ts.as_bytes().get(10) == Some(&b'T') && ts.ends_with('Z');
    Ok(if canonical {
        ts.to_owned()
    } else {
        a2a_protocol_types::unix_millis_to_iso8601(millis)
    })
}
