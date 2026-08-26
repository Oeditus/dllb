class Dllb < Formula
  desc "Multi-model NoSQL DBMS in Rust (Documents, Graphs, Search, Vectors)"
  homepage "https://github.com/Oeditus/dllb"
  url "https://github.com/Oeditus/dllb/archive/refs/tags/v1.0.0.tar.gz"
  sha256 "0000000000000000000000000000000000000000000000000000000000000000" # Update on release
  license "MIT"
  head "https://github.com/Oeditus/dllb.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args(path: "crates/server")
    system "cargo", "install", *std_cargo_args(path: "crates/cli")
  end

  test do
    assert_match "dllb", shell_output("#{bin}/dllb-cli --version")
  end
end
