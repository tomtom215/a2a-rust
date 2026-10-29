<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# TLS, proxies and compression

How the exporter reaches a collector that is not a plain `http://` port on
the same network: over TLS with a public or private CA, with a client
certificate, through an HTTP proxy, and with compressed bodies. Each setting
is also in the [Environment variables](./environment.md) matrix.

## TLS

`OTEL_EXPORTER_OTLP_ENDPOINT=https://collector.example:4318`. A certificate
from a public CA needs nothing more: the exporter trusts Mozilla's roots and
never reads the operating system's store, so a CA added to `/etc/ssl/certs`
changes nothing.

| You have | Set |
|---|---|
| A private CA | `OTEL_EXPORTER_OTLP_CERTIFICATE=/etc/otel/ca.pem` (a PEM file, trusted in addition to the Mozilla roots), or `with_ca_certificate_pem` in code |
| A collector that requires a client certificate | `OTEL_EXPORTER_OTLP_CLIENT_CERTIFICATE` and `OTEL_EXPORTER_OTLP_CLIENT_KEY`, also PEM files, or `with_client_certificate_pem` |
| A self-signed collector certificate | Make it with `-addext basicConstraints=critical,CA:FALSE` (and the collector's host in `subjectAltName`), then name the certificate itself in `OTEL_EXPORTER_OTLP_CERTIFICATE` |
| An endpoint by IP address | The address in the certificate's `subjectAltName` |

A self-signed certificate is refused if it is marked a CA, which
`openssl req -x509` does unless told otherwise; hence the `CA:FALSE` above.

An unreadable certificate or key file stops the process at startup, naming
the variable; a file that is not PEM, or (on OTLP/HTTP) a key that is not the
certificate's, fails `build`. Without the CA, the warning that follows a lost
export says what to set; so it does for a certificate for another host name,
an expired one, a collector that wants a client certificate and got none or
one it does not trust, and an `http://`/`https://` scheme that does not match
the receiver ([Debugging telemetry](./debugging.md#when-something-is-wrong)
shows the warning).

### An endpoint needs its scheme over OTLP/HTTP

`127.0.0.1:4318` is an error that says to write `http://127.0.0.1:4318` or
`https://…`. OTLP/gRPC reads a bare `host:port` in
`OTEL_EXPORTER_OTLP_ENDPOINT` as `https://`, or as `http://` when
`OTEL_EXPORTER_OTLP_INSECURE=true`; in code, give `with_otlp_endpoint` a full
URL.

## Proxies

`HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY` and `NO_PROXY` are read in upper or
lower case, and the upper-case one wins when both are set (curl reads
`http_proxy` in lower case only and prefers lower case, so a process that sets
both differently behaves differently here and in curl). The startup line says
`via proxy` and names the proxy, without its login.

* **How it is sent.** A collector at an `https://` address is reached through
  an `HTTP CONNECT` tunnel with TLS inside it, so its certificate is still
  checked against your CAs. A plain `http://` collector over OTLP/HTTP is sent
  as a forward-proxy request, with `Proxy-Authorization` from the login in the
  proxy's URL; over OTLP/gRPC it is tunnelled too.
* **A login** goes in the proxy's URL (`http://user:password@proxy:3128`); the
  startup line and the warnings show the proxy without it. When the proxy
  refuses, the warning says it was the proxy.
* **Only `http://` proxies are spoken.** A `socks5://` or `https://` value is
  ignored with one warning and the export goes direct, rather than the process
  refusing to start over a variable set for every other tool it runs.
* **In code**, `with_proxy(url)` names one proxy for every destination and,
  being explicit, does not consult `NO_PROXY`; `without_proxy()` ignores the
  environment.

### `NO_PROXY` and an in-cluster collector

A proxy cannot reach a collector inside the cluster, so exempt it with
`NO_PROXY`. It is a comma list of hosts, domain suffixes, CIDR ranges and `*`.
With `HTTP_PROXY` pointing at nothing, a collector at
`otel-collector.observability.svc.cluster.local` was:

| `NO_PROXY` | Result |
|---|---|
| `.svc` | Sent through the proxy, and lost: `.svc` does not match `x.ns.svc.cluster.local` |
| `.cluster.local` | Direct |
| `.svc.cluster.local` | Direct |
| `.svc,.cluster.local,10.0.0.0/8` | Direct |

A CIDR entry matches a collector addressed by IP and never one addressed by
name, even a name that resolves into the range. An IPv6 address may be
written bare or in brackets (`::1`, `[::1]`, `fd00::/8`).

A pod that sets `HTTP_PROXY` for its other traffic sends its export through
the proxy too unless `NO_PROXY` exempts the collector; check that when
upgrading from a release before 0.15.1, which read none of these variables
([upgrade guide](../../reference/upgrading.md)).

## Compression

`OTEL_EXPORTER_OTLP_COMPRESSION=gzip` (and the per-signal forms) sends bodies
as `Content-Encoding: gzip` over OTLP/HTTP and `grpc-encoding: gzip` over
OTLP/gRPC, and the startup line says `gzip`. On `hello-agent` the first traces
export went from 6,031 to 1,607 bytes and the first metrics export from 2,484
to 1,080 (body bytes on the wire, one run each; sizes vary a little with the
process). `with_compression(OtlpCompression::Gzip)` asks for it in code; code
cannot turn off what the variable turned on. `zstd` is an error, because it
needs a C library.

## How this was tested

On 2026-10-28, `tck/sut` exported all three signals over OTLP/HTTP and
OTLP/gRPC to otelcol-contrib 0.163.2 with: server TLS from a private CA, mutual
TLS (`client_ca_file`), a self-signed certificate marked CA and one marked
`CA:FALSE`, an expired certificate, a host name the certificate does not
list, an IP address it does, and `http://` against a TLS receiver and the
reverse; then through Squid 6.13 (forward requests for plaintext OTLP/HTTP,
`CONNECT` tunnels for TLS and for gRPC, mutual TLS inside a tunnel), with
`NO_PROXY` exempting the collector, a proxy that is not running, one that
denies everything, and one that requires a login, given it, not given it and
given a wrong one. Every working case delivered traces, metrics and logs
(read from the collector's `debug` exporter, Squid's access log and the
compose stack's backends), and every failing case ended with a warning
naming its cause. Five of those warnings had no hint, and three hints never
fired because they matched words rustls does not print; both are fixed, with
the failures' own text as their tests
(`crates/a2a-protocol-server/src/otel/telemetry/hints/tests.rs`).

Not run: IPv6 on the wire (the `NO_PROXY` handling of IPv6 addresses is
unit-tested) and an endpoint with a public CA.
