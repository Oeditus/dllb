class Dllb < Formula
  desc "Multi-model NoSQL DBMS in Rust (Documents, Graphs, Search, Vectors)"
  homepage "https://github.com/Oeditus/dllb"
  url "https://github.com/Oeditus/dllb/archive/refs/tags/v1.2.0.tar.gz"
  sha256 "ee28203025b7a0a700a92bb397f2b4e8fe8149bbba5935297bb7e93de0c135e0"
  license "MIT"
  head "https://github.com/Oeditus/dllb.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args(path: "crates/server")
    system "cargo", "install", *std_cargo_args(path: "crates/cli")
  end

  test do
    assert_match "dllb v#{version}", pipe_output("#{bin}/dllb-cli --no-color --path #{testpath}/test.redb", ".quit\n", 0)
    # `--version` must actually report the version; the CLI exits without
    # starting the REPL when it is passed.
    assert_match "dllb-cli", shell_output("#{bin}/dllb-cli --version")
    assert_match version.to_s, shell_output("#{bin}/dllb-cli --version")
  end
end
