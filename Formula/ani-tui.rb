class AniTui < Formula
  desc "Terminal UI for anime (now evolved into Solus)"
  homepage "https://github.com/abdullah-binmadhi/Solus"
  version "1.0.6"

  if Hardware::CPU.arm?
    url "https://github.com/abdullah-binmadhi/Solus/releases/download/v1.0.6/solus-macos-aarch64"
  else
    url "https://github.com/abdullah-binmadhi/Solus/releases/download/v1.0.6/solus-macos-x86_64"
  end

  def install
    bin.install Dir["solus-macos-*"].first => "ani-tui"
    bin.install_symlink bin/"ani-tui" => "solus"
  end

  def caveats
    <<~EOS
      ani-tui has evolved into Solus! The binary is available as both `solus` and `ani-tui`.

      Dependencies for media playback:
        brew install mpv ani-cli
    EOS
  end

  test do
    assert_predicate bin/"ani-tui", :exist?
  end
end
