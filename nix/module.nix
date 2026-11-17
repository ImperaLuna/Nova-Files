# The NixOS module: `programs.nova-files.enable` installs Nova Files in one
# of three modes (`programs.nova-files.mode`):
#
# - `launcher`: only the D-Bus service launchers search through (Search1),
#   started on demand. No app in the app list, no folder default, no file
#   picker.
# - `basic`: the app, the default for folders and the file picker of other
#   apps' Open and Save dialogs, with nothing else installed.
# - `full`: basic, plus what makes the extras work: video thumbnails
#   (ffmpegthumbnailer, found through the system's `share/thumbnailers`) and
#   drives in the sidebar (udisks2).
#
# `filePicker.enable` and `defaultFileManager` follow the mode and can still
# be set on their own.
#
# xdg-desktop-portal reads one portals.conf per desktop
# (`<desktop>-portals.conf`, else `portals.conf`), so the picker is set in
# each file `filePicker.desktops` names. Each such file replaces the one the
# desktop's own portal ships, so its other lines (`default` above all) must
# be set in `xdg.portal.config` too.
{ packages, builds }:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.programs.nova-files;
  system = pkgs.stdenv.hostPlatform.system;
  inherit (builds.${system}) appId portalName;
  app = cfg.mode != "launcher";

  # The launcher mode's install: the program and the D-Bus service that
  # starts it for a search, without the desktop entry, icon or portal file
  # that would list it as an app or offer it as a picker.
  service = pkgs.runCommand "nova-files-service" { } ''
    mkdir -p $out/bin $out/share/dbus-1/services
    ln -s ${cfg.package}/bin/nova-files $out/bin/nova-files
    ln -s ${cfg.package}/share/dbus-1/services/${appId}.service \
      $out/share/dbus-1/services/${appId}.service
  '';
  installed = if app then cfg.package else service;
in
{
  options.programs.nova-files = {
    enable = lib.mkEnableOption "Nova Files";
    mode = lib.mkOption {
      type = lib.types.enum [
        "launcher"
        "basic"
        "full"
      ];
      default = "basic";
      description = ''
        How much of Nova Files to install: `launcher` for only the search
        service launchers use, `basic` for the app as the folder default and
        file picker, `full` for basic plus video thumbnails and drives.
      '';
    };
    package = lib.mkOption {
      type = lib.types.package;
      default = packages.${system}.default;
      defaultText = lib.literalMD "the package from this flake";
      description = "The Nova Files package.";
    };
    defaultFileManager = lib.mkOption {
      type = lib.types.bool;
      default = app;
      defaultText = lib.literalExpression ''config.programs.nova-files.mode != "launcher"'';
      description = "Whether Nova Files opens folders (`inode/directory`) for every app.";
    };
    filePicker = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = app;
        defaultText = lib.literalExpression ''config.programs.nova-files.mode != "launcher"'';
        description = "Whether Nova Files is the file picker of other apps' Open and Save dialogs.";
      };
      desktops = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [ "common" ];
        example = [ "hyprland" ];
        description = ''
          The `xdg.portal.config` entries to set the picker in: `common` for
          `portals.conf`, or a desktop as `XDG_CURRENT_DESKTOP` names it, in
          lower case, for `<desktop>-portals.conf`, which wins over
          `portals.conf` on that desktop.
        '';
      };
    };
  };

  config = lib.mkIf cfg.enable (
    lib.mkMerge [
      {
        assertions = [
          {
            assertion = app || !(cfg.filePicker.enable || cfg.defaultFileManager);
            message = "programs.nova-files: the launcher mode installs no app, so it can't be the file picker or the folder default.";
          }
        ];
        environment.systemPackages = [ installed ];
        services.dbus.packages = [ installed ];
      }
      (lib.mkIf cfg.defaultFileManager {
        xdg.mime.defaultApplications."inode/directory" = lib.mkDefault "${appId}.desktop";
      })
      (lib.mkIf cfg.filePicker.enable {
        xdg.portal = {
          enable = true;
          extraPortals = [ cfg.package ];
          config = lib.genAttrs cfg.filePicker.desktops (_desktop: {
            "org.freedesktop.impl.portal.FileChooser" = [ portalName ];
          });
        };
      })
      (lib.mkIf (cfg.mode == "full") {
        environment.systemPackages = [ pkgs.ffmpegthumbnailer ];
        environment.pathsToLink = [ "/share/thumbnailers" ];
        services.udisks2.enable = true;
      })
    ]
  );
}
