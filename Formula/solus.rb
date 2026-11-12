class Solus < Formula
  desc "High-definition terminal anime theatre, discovery crucible, and AniList companion"
  homepage "https://github.com/abdullah-binmadhi/Solus"
  version "1.0.6"

  if Hardware::CPU.arm?
    url "https://github.com/abdullah-binmadhi/Solus/releases/download/v1.0.6/solus-macos-aarch64"
  else
    url "https://github.com/abdullah-binmadhi/Solus/releases/download/v1.0.6/solus-macos-x86_64"
  end

  def install
    bin.install Dir["solus-macos-*"].first => "solus"
  end

  def caveats
    <<~EOS
      Solus requires mpv and ani-cli for media playback:

        brew install mpv ani-cli

      For the best experience with full cover art and episode previews, run Solus in Kitty or WezTerm:
        brew install --cask kitty

      In the app:
      - press s for Settings & AniList Sync
      - press ! for Setup / dependency status
      - press ? for keyboard shortcuts
    EOS
  end

  test do
    assert_predicate bin/"solus", :exist?
  end
end
