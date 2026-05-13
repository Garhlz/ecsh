{
  description = "ecsh Rust development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  };

  outputs = { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];

      forAllSystems = f:
        nixpkgs.lib.genAttrs systems
          (system: f system);

      pkgsFor = system:
        import nixpkgs {
          inherit system;
        };
    in
    {
      devShells = forAllSystems (system:
        let
          pkgs = pkgsFor system;

          commonPackages = with pkgs; [
            rustup
            rust-analyzer
            lsof
          ];

          linuxPackages = with pkgs; [
            gdb
            valgrind
            strace
            pstree
          ];

          darwinPackages = with pkgs; [
            lldb
          ];
        in
        {
          default = pkgs.mkShell {
            packages =
              commonPackages
              ++ pkgs.lib.optionals pkgs.stdenv.isLinux linuxPackages
              ++ pkgs.lib.optionals pkgs.stdenv.isDarwin darwinPackages;
          };
        });
    };
}