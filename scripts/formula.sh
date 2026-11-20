#!/usr/bin/env bash
# Writes the Homebrew formula for a release to standard output.
#
# Usage: scripts/formula.sh <version> <directory with the .tar.gz.sha256 files of scripts/package.sh>
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <version> <checksum directory>" >&2
  exit 64
fi
version=$1
checksums=$2
repository="https://github.com/phranck/shotty"

# Prints the url and sha256 lines for one target.
asset() {
  local name="shotty-$version-$1.tar.gz"
  local file="$checksums/$name.sha256"
  if [[ ! -f $file ]]; then
    echo "error: $file is missing" >&2
    exit 1
  fi
  echo "      url \"$repository/releases/download/v$version/$name\""
  echo "      sha256 \"$(cut -d ' ' -f 1 "$file")\""
}

cat <<EOF
class Shotty < Formula
  desc "Turn the output of any command into a PNG or JPG image"
  homepage "$repository"
  version "$version"
  license "MIT"

  on_macos do
    on_arm do
$(asset aarch64-apple-darwin)
    end
    on_intel do
$(asset x86_64-apple-darwin)
    end
  end

  on_linux do
    on_arm do
$(asset aarch64-unknown-linux-gnu)
    end
    on_intel do
$(asset x86_64-unknown-linux-gnu)
    end
  end

  def install
    bin.install "shotty"
    man1.install Dir["man/*.1"]
    generate_completions_from_executable(bin/"shotty", "completions")
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/shotty version")
    pipe_output("#{bin}/shotty -o #{testpath}/out.png", "hello\\n")
    assert_path_exists testpath/"out.png"
  end
end
EOF
