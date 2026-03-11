{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.zeroclaw;
  settingsFormat = pkgs.formats.toml { };
  tomlConfig = settingsFormat.generate "zeroclaw.toml" cfg.settings;
in
{
  options.services.zeroclaw = {
    enable = lib.mkEnableOption "ZeroClaw AI assistant";

    package = lib.mkPackageOption pkgs "zeroclaw" { };

    user = lib.mkOption {
      type = lib.types.str;
      default = "zeroclaw";
      description = "User account under which ZeroClaw runs.";
    };

    group = lib.mkOption {
      type = lib.types.str;
      default = "zeroclaw";
      description = "Group under which ZeroClaw runs.";
    };

    settings = lib.mkOption {
      type = lib.types.submodule {
        freeformType = settingsFormat.type;
      };
      default = { };
      description = ''
        ZeroClaw configuration as Nix attribute set.
        See https://github.com/aieos/zeroclaw for options.
      '';
    };

    credentials = lib.mkOption {
      type = lib.types.attrsOf lib.types.path;
      default = { };
      example = lib.literalExpression ''
        {
          LONGCAT_API_KEY = config.sops.secrets.longcat-api-key.path;
        }
      '';
      description = "Credential files to load via systemd LoadCredential.";
    };

    mode = lib.mkOption {
      type = lib.types.enum [
        "gateway"
        "daemon"
        "agent"
      ];
      default = "gateway";
      description = "Runtime mode.";
    };

    openFirewall = lib.mkEnableOption "firewall port for gateway";

    accessiblePaths = lib.mkOption {
      type = lib.types.listOf (
        lib.types.submodule {
          options = {
            path = lib.mkOption {
              type = lib.types.path;
              description = "Directory or file path to allow access.";
            };
            mode = lib.mkOption {
              type = lib.types.enum [
                "read-only"
                "read-write"
              ];
              default = "read-only";
              description = "Access mode for this path.";
            };
          };
        }
      );
      default = [ ];
      example = lib.literalExpression ''
        [
          { path = "/var/lib/zeroclaw/workspace"; mode = "read-write"; }
          { path = "/etc/nixos"; mode = "read-only"; }
        ]
      '';
      description = ''
        Additional paths to make accessible to the ZeroClaw service.
        By default, the service can only access its state directory.
      '';
    };

    path = lib.mkOption {
      type = lib.types.listOf lib.types.package;
      default = [ ];
      example = lib.literalExpression "[ pkgs.git pkgs.curl pkgs.jq ]";
      description = "Packages to add to the service's PATH.";
    };
    environment = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { };
      example = lib.literalExpression ''
        {
          HOME = "/var/lib/zeroclaw";
          USER = "zeroclaw";
          SHELL = "''${pkgs.bash}/bin/bash";
        }
      '';
      description = "Additional environment variables for the service.";
    };
  };

  config = lib.mkIf cfg.enable {
    users.users.${cfg.user} = lib.mkIf (cfg.user == "zeroclaw") {
      isSystemUser = true;
      group = cfg.group;
      home = "/var/lib/zeroclaw";
    };
    users.groups.${cfg.group} = lib.mkIf (cfg.group == "zeroclaw") { };

    systemd.services.zeroclaw = {
      description = "ZeroClaw AI Assistant";
      after = [ "network.target" ];
      wantedBy = [ "multi-user.target" ];

      script = ''
        mkdir -p /var/lib/zeroclaw/.zeroclaw
        cp ${tomlConfig} /var/lib/zeroclaw/.zeroclaw/config.toml
        chmod 600 /var/lib/zeroclaw/.zeroclaw/config.toml

        ${lib.concatMapStrings (name: ''
          export ${name}=$(cat "$CREDENTIALS_DIRECTORY/${name}")
        '') (lib.attrNames cfg.credentials)}

        exec ${lib.getExe cfg.package} --config-dir /var/lib/zeroclaw/.zeroclaw ${cfg.mode}
      '';

      path = cfg.path;
      environment = {
        HOME = "/var/lib/zeroclaw";
        USER = cfg.user;
        SHELL = "${pkgs.bash}/bin/bash";
      }
      // cfg.environment;

      serviceConfig = {
        Type = "simple";
        DynamicUser = true;
        User = cfg.user;
        Group = cfg.group;
        StateDirectory = "zeroclaw";
        WorkingDirectory = "/var/lib/zeroclaw";
        StateDirectoryMode = "0700";

        Restart = "on-failure";
        RestartSec = 10;

        LoadCredential = lib.mapAttrsToList (name: path: "${name}:${path}") cfg.credentials;

        # Filesystem
        ProtectSystem = "strict";
        ProtectHome = true;
        ReadWritePaths = [
          "/var/lib/zeroclaw"
        ]
        ++ lib.map (p: p.path) (lib.filter (p: p.mode == "read-write") cfg.accessiblePaths);
        ReadOnlyPaths = lib.map (p: p.path) (lib.filter (p: p.mode == "read-only") cfg.accessiblePaths);
        BindReadOnlyPaths = lib.map (p: "${p.path}:${p.path}") (
          lib.filter (p: p.mode == "read-only") cfg.accessiblePaths
        );

        PrivateTmp = true;

        # Devices
        PrivateDevices = true;
        DeviceAllow = [ ];

        # Kernel
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;

        # Capabilities
        CapabilityBoundingSet = [ "" ];
        NoNewPrivileges = true;

        # Execution
        ProtectClock = true;
        ProtectHostname = true;
        ProtectProc = "invisible";
        ProcSubset = "pid";
        RestrictSUIDSGID = true;
        RestrictRealtime = true;
        RestrictNamespaces = true;
        LockPersonality = true;
        # disabled for v8 JIT
        MemoryDenyWriteExecute = false;

        # Sandboxing
        SystemCallFilter = [
          "@system-service"
          "~@privileged"
        ];
        SystemCallArchitectures = "native";
      };
    };

    networking.firewall = lib.mkIf cfg.openFirewall {
      allowedTCPPorts = [ (cfg.settings.gateway.port or 8080) ];
    };
  };
}
