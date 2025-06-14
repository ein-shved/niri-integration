{
  description = ''
    A glue-utility between niri and environment like kitty and vim
  '';

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";

  outputs =
    { nixpkgs, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
        };
        niri-integration = pkgs.callPackage ./. { };
      in
      {
        packages = {
          inherit niri-integration;
          default = niri-integration;
        };
        formatter = pkgs.nixfmt-rfc-style;
        devShells.default = pkgs.mkShell {
          inputsFrom = [ niri-integration ];
          packages = with pkgs; [
            rust-analyzer
            rustfmt
          ];
        };
      }
    );
}
