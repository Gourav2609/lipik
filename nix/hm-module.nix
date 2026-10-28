# Home-manager module for Lipik speech-to-text
#
# Provides a systemd user service for autostart.
# Usage: imports = [ lipik.homeManagerModules.default ];
#        services.lipik.enable = true;
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.lipik;
in
{
  options.services.lipik = {
    enable = lib.mkEnableOption "Lipik speech-to-text user service";

    package = lib.mkOption {
      type = lib.types.package;
      defaultText = lib.literalExpression "lipik.packages.\${system}.lipik";
      description = "The Lipik package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.user.services.lipik = {
      Unit = {
        Description = "Lipik speech-to-text";
        After = [ "graphical-session.target" ];
        PartOf = [ "graphical-session.target" ];
      };
      Service = {
        ExecStart = "${cfg.package}/bin/lipik";
        Restart = "on-failure";
        RestartSec = 5;
      };
      Install.WantedBy = [ "graphical-session.target" ];
    };
  };
}
