# The NixOS module: `programs.nova-files.enable` installs Nova Files and its
# D-Bus services; `programs.nova-files.filePicker.enable` also makes it the
# file picker, through xdg-desktop-portal's FileChooser.
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
  inherit (builds.${system}) portalName;
in
{
  options.programs.nova-files = {
    enable = lib.mkEnableOption "Nova Files";
    package = lib.mkOption {
      type = lib.types.package;
      default = packages.${system}.default;
      defaultText = lib.literalMD "the package from this flake";
      description = "The Nova Files package.";
    };
    filePicker = {
      enable = lib.mkEnableOption "Nova Files as the file picker of other apps' Open and Save dialogs";
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
        environment.systemPackages = [ cfg.package ];
        services.dbus.packages = [ cfg.package ];
      }
      (lib.mkIf cfg.filePicker.enable {
        xdg.portal = {
          enable = true;
          extraPortals = [ cfg.package ];
          config = lib.genAttrs cfg.filePicker.desktops (_desktop: {
            "org.freedesktop.impl.portal.FileChooser" = [ portalName ];
          });
        };
      })
    ]
  );
}
