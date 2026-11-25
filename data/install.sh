#!/usr/bin/env bash
# Installs Nova Files' files under a prefix, on any distro: the programs,
# the D-Bus service files, the desktop entry and its icon, and the file
# picker's .portal file. The Nix package runs it too, so both installs
# have the same layout. `just install` and `just uninstall` call it.
#
# The mode picks how much is installed, as `programs.nova-files.mode` does
# in the NixOS module:
#   launcher  the programs and the D-Bus service launchers search through,
#             without the desktop entry, icon or picker that list it as an
#             app
#   basic     the app: also its desktop entry, icon, file picker and apps'
#             "Show in folder"
#   full      basic; the extras it uses come from the distro's packages
#             (ffmpegthumbnailer, udisks2), so it names any that are missing
# Installing a mode over another removes the files the new one leaves out.
#
# Usage: data/install.sh [--uninstall] [--mode MODE] --prefix PREFIX
#            --app-id ID --portal-bus-name NAME [--from DIR]
# --from is the folder holding the built programs; left out, the programs
# are taken as already in PREFIX/bin (the Nix package installs them first).
# --uninstall removes what any mode installs, and takes no mode.
# DESTDIR, when set, is put in front of every path written, for packagers;
# the files themselves name the paths under PREFIX.
set -euo pipefail

readonly PROGRAM=nova-files
readonly HELPER=nova-files-thumbnailer
readonly ARCHIVER=nova-files-archiver
readonly PLAYER=nova-files-player
# What `full` uses from the distro, as `program:package`.
readonly FULL_EXTRAS="ffmpegthumbnailer:ffmpegthumbnailer udisksctl:udisks2"
# The prefix is written into Exec lines, which split on spaces and give
# `%`, quotes and `$` a meaning, so it may hold only these.
readonly PREFIX_UNSAFE='[^A-Za-z0-9/._+@:~-]'

usage() {
    echo "usage: $0 [--uninstall] [--mode launcher|basic|full] --prefix PREFIX --app-id ID --portal-bus-name NAME [--from DIR]" >&2
    exit 2
}

fail() {
    echo "install.sh: $1" >&2
    exit 1
}

data=$(CDPATH='' cd -- "$(dirname -- "$0")" >/dev/null && pwd)
mode="" prefix="" app_id="" portal_bus_name="" from="" uninstall=false
while (($#)); do
    case $1 in
    --uninstall) uninstall=true ;;
    --mode | --prefix | --app-id | --portal-bus-name | --from)
        (($# >= 2)) || usage
        case $1 in
        --mode) mode=$2 ;;
        --prefix) prefix=$2 ;;
        --app-id) app_id=$2 ;;
        --portal-bus-name) portal_bus_name=$2 ;;
        --from) from=$2 ;;
        esac
        shift
        ;;
    *) usage ;;
    esac
    shift
done
if $uninstall; then
    mode=uninstall
fi
case $mode in
launcher | basic | full | uninstall) ;;
*) usage ;;
esac
[ -n "$prefix" ] && [ -n "$app_id" ] && [ -n "$portal_bus_name" ] || usage
[[ $prefix == /* ]] || fail "the prefix must be an absolute path: $prefix"
[[ ! $prefix =~ $PREFIX_UNSAFE ]] ||
    fail "the prefix may hold only letters, digits and / . _ + @ : ~ -: $prefix"

# xdg-desktop-portal's portals.conf picks a backend by the .portal file's
# name, the bus name's last part.
portal_name=${portal_bus_name##*.}
bin=$prefix/bin/$PROGRAM
root=${DESTDIR:-}$prefix

# Every file a mode may install, one per line: how it is made, the modes
# that install it (`all`, or `app` for basic and full), its source and its
# path under the prefix.
# - The programs. The window looks for the thumbnail and archive helpers
#   next to the program, and launcher mode opens windows too.
# - D-Bus starts the program in service mode for a launcher's search.
# - Launchers list the app from the desktop entry, named after the app id
#   as DBusActivatable requires. It names the icon by the app id, so
#   launchers and docks find it in hicolor whatever icon theme is set.
# - The file picker: xdg-desktop-portal reads the .portal file, and D-Bus
#   starts the program in service mode when the portal first calls it.
# - Apps' "Show in folder" (org.freedesktop.FileManager1): when no file
#   manager holds the name, D-Bus may start the program for it, which then
#   answers. The file is named after the app id, so it never clashes with
#   another file manager's file in a merged share folder; D-Bus still
#   picks one of the files that name it.
layout() {
    cat <<EOF
program all $PROGRAM bin/$PROGRAM
program all $HELPER bin/$HELPER
program all $ARCHIVER bin/$ARCHIVER
program all $PLAYER bin/$PLAYER
template all dbus-service.in share/dbus-1/services/$app_id.service
template app desktop-entry.in share/applications/$app_id.desktop
copy app app-icon.svg share/icons/hicolor/scalable/apps/$app_id.svg
template app portal.in share/xdg-desktop-portal/portals/$portal_name.portal
template app portal-dbus-service.in share/dbus-1/services/$portal_bus_name.service
template app file-manager-dbus-service.in share/dbus-1/services/$app_id.FileManager1.service
EOF
}

# Escapes `text` for the replacement side of a sed `s|||` command.
replacement() {
    printf '%s' "$1" | sed -e 's/[\\|&]/\\&/g'
}

# Copies a data template to `destination`, filling in its @names@. A name
# left unfilled is an error, so a new one can't ship as literal text.
template() {
    local source=$1 destination=$2
    install -Dm644 "$data/$source" "$destination"
    sed -i \
        -e "s|@bin@|$(replacement "$bin")|g" \
        -e "s|@app_id@|$(replacement "$app_id")|g" \
        -e "s|@portal_bus_name@|$(replacement "$portal_bus_name")|g" \
        "$destination"
    if grep -q '@[a-z_]*@' "$destination"; then
        fail "$destination has a name install.sh doesn't fill"
    fi
}

# Installs a built program, unless it is already in place.
program() {
    local name=$1 destination=$2
    if [ -n "$from" ]; then
        install -Dm755 "$from/$name" "$destination"
    elif [ ! -e "$destination" ]; then
        fail "$destination is missing; pass --from with the built programs"
    fi
}

# Whether this mode installs a file marked `modes`.
wanted() {
    case $mode/$1 in
    launcher/all | basic/* | full/*) return 0 ;;
    *) return 1 ;;
    esac
}

while read -r kind modes source path; do
    destination=$root/$path
    if ! wanted "$modes"; then
        rm -f -- "$destination"
        continue
    fi
    case $kind in
    program) program "$source" "$destination" ;;
    template) template "$source" "$destination" ;;
    copy) install -Dm644 "$data/$source" "$destination" ;;
    *) fail "unknown kind in the layout: $kind" ;;
    esac
done < <(layout)

if [ "$mode" = full ]; then
    missing=""
    for extra in $FULL_EXTRAS; do
        command -v "${extra%%:*}" >/dev/null || missing+=" ${extra#*:}"
    done
    if [ -n "$missing" ]; then
        echo "Full mode also uses these packages, which aren't installed:$missing" >&2
    fi
fi
