{
  description = "Miniconf";
  inputs.nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
  outputs =
    { self, nixpkgs }:
    let
      pkgs = import nixpkgs { system = "x86_64-linux"; };
      aiomqtt22 = pkgs.python3Packages.aiomqtt.overrideAttrs rec {
        version = "2.2.0";
        src = pkgs.fetchFromGitHub {
          owner = "sbtinstruments";
          repo = "aiomqtt";
          rev = "refs/tags/v${version}";
          hash = "sha256-Sn9wGN93g61tPxuUZbGuElBXqnMEzJilfl3uvnKdIG0=";
        };
        propagatedBuildInputs = [
          pkgs.python3Packages.paho-mqtt_2
          pkgs.python3Packages.typing-extensions
        ];
      };
      miniconf-mqtt-py = pkgs.python3Packages.buildPythonPackage {
        pname = "miniconf";
        version = "0.18.0";
        src = self + "/py/miniconf-mqtt";
        format = "pyproject";
        buildInputs = [
          pkgs.python3Packages.setuptools
        ];
        propagatedBuildInputs = [
          # pkgs.python3Packages.aiomqtt
          aiomqtt22
          pkgs.python3Packages.typing-extensions
        ];
        # checkPhase = "python -m miniconf";
      };
    in
    {
      packages.x86_64-linux = {
        inherit miniconf-mqtt-py aiomqtt22;
        default = miniconf-mqtt-py;
      };
      devShells.x86_64-linux.default = pkgs.mkShellNoCC {
        name = "miniconf-dev-shell";
        packages = [ miniconf-mqtt-py ];
      };
    };
}
