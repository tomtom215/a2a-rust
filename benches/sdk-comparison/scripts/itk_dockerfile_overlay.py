# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Writes Dockerfile.ccr next to a2a-itk's Dockerfile: the upstream file with
environment-only lines inserted so the image can build behind an HTTPS proxy
that re-terminates TLS. No test logic, scenario, or runner file is touched.

usage: itk_dockerfile_overlay.py <a2a-itk checkout> <proxy url> <ca bundle path>
Then: cp <ca bundle> <checkout>/ccr-ca.crt
      docker build --network host -f Dockerfile.ccr -t itk_service <checkout>
Not needed on a machine with direct internet access: build the upstream
Dockerfile instead."""
import os, sys

itk, proxy, _ca = sys.argv[1:4]
noproxy = os.environ.get("NO_PROXY", "localhost,127.0.0.1")
s = open(os.path.join(itk, "Dockerfile")).read()
ca = "/usr/local/share/ca-certificates/ccr-ca.crt"
host, port = proxy.split("//")[1].rstrip("/").split(":")
inject = f'''
# ---- environment-only overlay for a proxied sandbox (not part of upstream) ----
COPY ccr-ca.crt {ca}
ENV HTTPS_PROXY={proxy} https_proxy={proxy} \\
    NO_PROXY="{noproxy}" no_proxy="{noproxy}" \\
    SSL_CERT_FILE={ca} \\
    CURL_CA_BUNDLE={ca} \\
    REQUESTS_CA_BUNDLE={ca} \\
    NODE_EXTRA_CA_CERTS={ca} \\
    CARGO_HTTP_CAINFO={ca} \\
    GIT_SSL_CAINFO={ca} \\
    UV_NATIVE_TLS=1 \\
    JAVA_TOOL_OPTIONS="-Dhttps.proxyHost={host} -Dhttps.proxyPort={port}"
RUN sed -i 's|http://deb.debian.org|https://deb.debian.org|g' /etc/apt/sources.list.d/debian.sources \\
 && echo 'Acquire::https::Proxy "{proxy}";' > /etc/apt/apt.conf.d/99proxy \\
 && echo 'Acquire::https::CaInfo "{ca}";' >> /etc/apt/apt.conf.d/99proxy
# ---- end overlay ----
'''
anchor = 'SHELL ["/bin/bash", "-o", "pipefail", "-c"]\n'
assert s.count(anchor) == 1, "upstream Dockerfile changed shape; re-derive the overlay"
s = s.replace(anchor, anchor + inject, 1)
go = "# Install Go 1.25.0"
assert s.count(go) == 1
s = s.replace(go, f"RUN keytool -importcert -noprompt -alias ccr -file {ca} -cacerts -storepass changeit\n\n" + go, 1)
open(os.path.join(itk, "Dockerfile.ccr"), "w").write(s)
