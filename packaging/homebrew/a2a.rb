# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# DRAFT — not in any tap. See packaging/README.md. Replace every @NAME@
# placeholder from a release whose binaries `release.yml` built and attested.
class A2a < Formula
  desc "Command-line client for A2A agents: call, sign and verify cards, verify audit chains"
  homepage "https://a2a-rust.com"
  version "@VERSION@"
  license "Apache-2.0"

  on_macos do
    on_arm do
      url "https://github.com/tomtom215/a2a-rust/releases/download/v#{version}/a2a-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "@SHA256_AARCH64_APPLE_DARWIN@"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/tomtom215/a2a-rust/releases/download/v#{version}/a2a-v#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "@SHA256_X86_64_UNKNOWN_LINUX_GNU@"
    end
    on_arm do
      url "https://github.com/tomtom215/a2a-rust/releases/download/v#{version}/a2a-v#{version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "@SHA256_AARCH64_UNKNOWN_LINUX_GNU@"
    end
  end

  def install
    bin.install "a2a"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/a2a --version")
    # An offline command, so the test needs no network.
    (testpath/"records.json").write("[]")
    output = shell_output("#{bin}/a2a audit verify #{testpath}/records.json")
    assert_match "\"intact\": true", output
  end
end
