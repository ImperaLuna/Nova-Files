# Evaluates the NixOS module (nix/module.nix) in each mode and checks what
# it installs and sets, without building Nova Files: a stand-in package
# with the same file layout takes its place.
{ pkgs, build }:
let
  inherit (pkgs) lib;
  inherit (build) appId portalName;
  fileChooser = "org.freedesktop.impl.portal.FileChooser";

  standIn = pkgs.runCommand "nova-files-stand-in" { } ''
    mkdir -p $out/bin $out/share/dbus-1/services $out/share/applications
    touch $out/bin/nova-files $out/share/dbus-1/services/${appId}.service
    touch $out/share/applications/${appId}.desktop
  '';
  module = import ./module.nix {
    packages.${pkgs.stdenv.hostPlatform.system}.default = standIn;
    builds.${pkgs.stdenv.hostPlatform.system} = build;
  };

  # The system's config with `settings` for programs.nova-files.
  system =
    settings:
    (pkgs.nixos {
      imports = [ module ];
      programs.nova-files = {
        enable = true;
      }
      // settings;
      system.stateVersion = lib.trivial.release;
      # No boot loader or file systems to check.
      boot.isContainer = true;
    }).config;

  installs =
    config: name: lib.any (package: lib.getName package == name) config.environment.systemPackages;
  folderDefault = config: config.xdg.mime.defaultApplications."inode/directory" or null;
  # portals.conf takes the list as one `;`-joined string.
  picker = config: config.xdg.portal.config.common.${fileChooser} or null;
  failed = config: lib.any (check: !check.assertion) config.assertions;

  launcher = system { mode = "launcher"; };
  basic = system { mode = "basic"; };
  full = system { mode = "full"; };
  launcherWithPicker = system {
    mode = "launcher";
    filePicker.enable = true;
  };
  launcherOpeningFolders = system {
    mode = "launcher";
    defaultFileManager = true;
  };

  expectations = {
    "launcher installs only the search service" =
      installs launcher "nova-files-service" && !installs launcher "nova-files-stand-in";
    "launcher sets no folder default or picker" =
      folderDefault launcher == null && picker launcher == null && !failed launcher;
    "launcher can't be the picker or the folder default" =
      failed launcherWithPicker && failed launcherOpeningFolders;
    "basic installs the app" = installs basic "nova-files-stand-in";
    "basic opens folders and picks files" =
      folderDefault basic == "${appId}.desktop" && picker basic == portalName;
    "basic adds nothing more" =
      !installs basic "ffmpegthumbnailer" && !basic.services.udisks2.enable && !failed basic;
    "full adds thumbnails and drives" =
      installs full "ffmpegthumbnailer"
      && full.services.udisks2.enable
      && picker full == portalName
      && folderDefault full == "${appId}.desktop"
      && !failed full;
  };
  broken = lib.attrNames (lib.filterAttrs (_name: holds: !holds) expectations);
in
pkgs.runCommand "nova-files-module" { } (
  if broken == [ ] then
    "touch $out"
  else
    ''
      echo "The NixOS module broke: ${lib.concatStringsSep "; " broken}" >&2
      exit 1
    ''
)
