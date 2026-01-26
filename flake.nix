{
  description = "pqcoin - Post-quantum cryptocurrency";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.11";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};

        pqcoin = pkgs.rustPlatform.buildRustPackage {
          pname = "pqcoin";
          version = "0.1.0";
          src = ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
            allowBuiltinFetchGit = true;
          };

          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = [ pkgs.openssl ]
            ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
              pkgs.darwin.apple_sdk.frameworks.Security
              pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
            ];

          # Skip tests during build
          doCheck = false;

          meta = with pkgs.lib; {
            description = "Post-quantum cryptocurrency node";
            homepage = "https://github.com/SamuelSchlesinger/pqcoin";
            license = licenses.mit;
            mainProgram = "pqcoin";
          };
        };

      in {
        packages = {
          default = pqcoin;
          pqcoin = pqcoin;
        };

        apps.default = {
          type = "program";
          program = "${pqcoin}/bin/pqcoin";
        };

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            cargo
            rustc
            pkg-config
            openssl
          ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
            darwin.apple_sdk.frameworks.Security
            darwin.apple_sdk.frameworks.SystemConfiguration
          ];
        };
      }
    ) // {
      # NixOS module
      nixosModules.default = { config, pkgs, lib, ... }:
        let
          cfg = config.services.pqcoin;
          pqcoinPkg = self.packages.${pkgs.system}.pqcoin;
        in {
          options.services.pqcoin = {
            enable = lib.mkEnableOption "pqcoin node";

            testnet = lib.mkOption {
              type = lib.types.bool;
              default = true;
              description = "Run on testnet";
            };

            mine = lib.mkOption {
              type = lib.types.bool;
              default = false;
              description = "Enable mining";
            };

            rpc = {
              enable = lib.mkOption {
                type = lib.types.bool;
                default = true;
                description = "Enable JSON-RPC API";
              };

              bind = lib.mkOption {
                type = lib.types.str;
                default = "0.0.0.0";
                description = "RPC bind address";
              };

              port = lib.mkOption {
                type = lib.types.port;
                default = 8332;
                description = "RPC port";
              };
            };

            p2p = {
              port = lib.mkOption {
                type = lib.types.port;
                default = 8333;
                description = "P2P port";
              };

              seedPeers = lib.mkOption {
                type = lib.types.listOf lib.types.str;
                default = [];
                description = "Seed peers";
              };
            };

            logLevel = lib.mkOption {
              type = lib.types.enum [ "trace" "debug" "info" "warn" "error" ];
              default = "info";
              description = "Log level";
            };
          };

          config = lib.mkIf cfg.enable {
            users.users.pqcoin = {
              isSystemUser = true;
              group = "pqcoin";
              home = "/var/lib/pqcoin";
              createHome = true;
            };
            users.groups.pqcoin = {};

            systemd.services.pqcoin = {
              description = "pqcoin node";
              wantedBy = [ "multi-user.target" ];
              after = [ "network.target" ];

              serviceConfig = {
                Type = "simple";
                User = "pqcoin";
                Group = "pqcoin";
                StateDirectory = if cfg.testnet then "pqcoin-testnet" else "pqcoin";
                ExecStart = lib.concatStringsSep " " (
                  [ "${pqcoinPkg}/bin/pqcoin" ]
                  ++ lib.optional cfg.testnet "--testnet"
                  ++ lib.optional cfg.mine "--mine"
                  ++ lib.optionals cfg.rpc.enable [
                    "--rpc" "--rpc-bind" cfg.rpc.bind "--rpc-port" (toString cfg.rpc.port)
                  ]
                  ++ [ "--port" (toString cfg.p2p.port) "--log-level" cfg.logLevel ]
                  ++ (map (p: "-C ${p}") cfg.p2p.seedPeers)
                );
                Restart = "on-failure";
                RestartSec = "10s";
              };
            };

            networking.firewall.allowedTCPPorts = [ cfg.p2p.port ]
              ++ lib.optional cfg.rpc.enable cfg.rpc.port;

            environment.systemPackages = [ pqcoinPkg ];
          };
        };
    };
}
