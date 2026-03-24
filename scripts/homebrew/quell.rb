# This formula is intended for a future Homebrew tap (e.g. homebrew-quell).
# For now it can be installed locally: brew install --formula scripts/homebrew/quell.rb
class Quell < Formula
  desc "Windows-native terminal proxy for AI CLI tools - eliminates scroll-jumping and flicker"
  homepage "https://github.com/FurbySoup/quell"
  version "0.1.1"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/FurbySoup/quell/releases/download/v#{version}/quell-macos-aarch64"
      sha256 "PLACEHOLDER_SHA256_AARCH64"
    elsif Hardware::CPU.intel?
      url "https://github.com/FurbySoup/quell/releases/download/v#{version}/quell-macos-x86_64"
      sha256 "PLACEHOLDER_SHA256_X86_64"
    end
  end

  def install
    binary = Dir["quell-*"].first || "quell"
    bin.install binary => "quell"
  end

  test do
    assert_match "quell", shell_output("#{bin}/quell --help")
  end
end
