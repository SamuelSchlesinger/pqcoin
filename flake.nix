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
      # NixOS module with multi-instance support
      nixosModules.default = { config, pkgs, lib, ... }:
        let
          pqcoinPkg = self.packages.${pkgs.system}.pqcoin;

          # Instance options submodule
          instanceOptions = { name, ... }: {
            options = {
              enable = lib.mkEnableOption "this pqcoin instance";

              network = lib.mkOption {
                type = lib.types.enum [ "mainnet" "testnet" ];
                default = "mainnet";
                description = "Network type (mainnet or testnet)";
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
                  default = "127.0.0.1";
                  description = "RPC bind address";
                };

                port = lib.mkOption {
                  type = lib.types.port;
                  default = if name == "testnet" then 18332 else 8332;
                  description = "RPC port";
                };

                metricsPort = lib.mkOption {
                  type = lib.types.port;
                  default = if name == "testnet" then 19091 else 9091;
                  description = "Metrics port";
                };
              };

              p2p = {
                port = lib.mkOption {
                  type = lib.types.port;
                  default = if name == "testnet" then 18333 else 8333;
                  description = "P2P port";
                };

                seedPeers = lib.mkOption {
                  type = lib.types.listOf lib.types.str;
                  default = [];
                  description = "Seed peers to connect to";
                };
              };

              logLevel = lib.mkOption {
                type = lib.types.enum [ "trace" "debug" "info" "warn" "error" ];
                default = "info";
                description = "Log level";
              };

              extraArgs = lib.mkOption {
                type = lib.types.listOf lib.types.str;
                default = [];
                description = "Extra command-line arguments";
              };

              openFirewall = lib.mkOption {
                type = lib.types.bool;
                default = true;
                description = "Open firewall ports for P2P and RPC";
              };
            };
          };

          # Helper to generate systemd service for an instance
          mkService = name: icfg: lib.mkIf icfg.enable {
            description = "pqcoin node (${name})";
            wantedBy = [ "multi-user.target" ];
            after = [ "network-online.target" ];
            wants = [ "network-online.target" ];

            serviceConfig = {
              Type = "simple";
              User = "pqcoin";
              Group = "pqcoin";
              StateDirectory = "pqcoin/${name}";

              ExecStart = lib.concatStringsSep " " (
                [ "${pqcoinPkg}/bin/pqcoin" ]
                ++ lib.optional (icfg.network == "testnet") "--testnet"
                ++ lib.optional icfg.mine "--mine"
                ++ lib.optionals icfg.rpc.enable [
                  "--rpc"
                  "--rpc-bind" icfg.rpc.bind
                  "--rpc-port" (toString icfg.rpc.port)
                ]
                ++ [ "--port" (toString icfg.p2p.port) ]
                ++ [ "--log-level" icfg.logLevel ]
                ++ [ "--datadir" "/var/lib/pqcoin/${name}/data" ]
                ++ (map (p: "-C ${p}") icfg.p2p.seedPeers)
                ++ icfg.extraArgs
              );

              Restart = "on-failure";
              RestartSec = "10s";

              # Security hardening
              NoNewPrivileges = true;
              PrivateTmp = true;
              ProtectSystem = "strict";
              ProtectHome = true;
              ReadWritePaths = [ "/var/lib/pqcoin/${name}" ];
              CapabilityBoundingSet = "";
              LockPersonality = true;
              MemoryDenyWriteExecute = true;
              PrivateDevices = true;
              ProtectClock = true;
              ProtectControlGroups = true;
              ProtectHostname = true;
              ProtectKernelLogs = true;
              ProtectKernelModules = true;
              ProtectKernelTunables = true;
              RestrictNamespaces = true;
              RestrictRealtime = true;
              RestrictSUIDSGID = true;
              SystemCallArchitectures = "native";

              # Resource limits
              LimitNOFILE = 65535;
            };
          };

          # Collect all firewall ports from enabled instances
          allPorts = lib.flatten (lib.mapAttrsToList (name: icfg:
            lib.optionals (icfg.enable && icfg.openFirewall) (
              [ icfg.p2p.port ]
              ++ lib.optional icfg.rpc.enable icfg.rpc.port
            )
          ) config.services.pqcoin.instances);

          # Check if any instance is enabled
          anyEnabled = lib.any (icfg: icfg.enable) (lib.attrValues config.services.pqcoin.instances);

        in {
          options.services.pqcoin = {
            instances = lib.mkOption {
              type = lib.types.attrsOf (lib.types.submodule instanceOptions);
              default = {};
              description = "pqcoin node instances";
              example = lib.literalExpression ''
                {
                  mainnet = {
                    enable = true;
                    network = "mainnet";
                    rpc.enable = true;
                    p2p.port = 8333;
                  };
                  testnet = {
                    enable = true;
                    network = "testnet";
                    mine = true;
                    p2p.port = 18333;
                  };
                }
              '';
            };
          };

          config = lib.mkIf anyEnabled {
            # Create pqcoin user/group
            users.users.pqcoin = {
              isSystemUser = true;
              group = "pqcoin";
              home = "/var/lib/pqcoin";
              createHome = true;
            };
            users.groups.pqcoin = {};

            # Create systemd services for each enabled instance
            systemd.services = lib.mapAttrs' (name: icfg:
              lib.nameValuePair "pqcoin-${name}" (mkService name icfg)
            ) config.services.pqcoin.instances;

            # Open firewall ports
            networking.firewall.allowedTCPPorts = allPorts;

            # Add pqcoin to system packages
            environment.systemPackages = [ pqcoinPkg ];
          };
        };
    };
}
