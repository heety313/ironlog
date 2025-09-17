# Note: This file is only needed for development on NixOS
{ pkgs ? import <nixpkgs> {}
}: pkgs.mkShell {
    buildInputs = with pkgs; [
        # Rust development
	rustup

        # Additional dependencies
        pkg-config
        cmake
        openssl
    ];
}
