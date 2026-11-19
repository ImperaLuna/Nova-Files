#!/usr/bin/env bash
# Fails when a crate depends on a crate it may not use.
#
# Two checks. Each Cargo.toml lists the outside and workspace crates it may
# use under its "# Allowed outside crates:" and "# Allowed workspace crates:"
# lines, one "# - name" line each, or "none" on the header line. The layer
# rules below come from each crate's folder, so a new crate can't opt out
# of them.
set -euo pipefail

allowed_list() { # <Cargo.toml> <label>
    awk -v header="# Allowed $2 crates:" '
        found && /^# - / { print substr($0, 5); next }
        found { exit }
        index($0, header) == 1 {
            found = 1
            rest = substr($0, length(header) + 1)
            gsub(/^[[:space:]]+|[[:space:]]+$/, "", rest)
            if (rest != "" && rest != "none") { print "BAD " rest }
        }
        END { if (!found) print "MISSING" }
    ' "$1" | tr '\n' ' '
}

root=$(pwd)

layer_of() { # <crate folder relative to the root>
    case $1 in
        app) echo app ;;
        legacy/*) echo legacy ;;
        fronts/*) echo front ;;
        helpers/*) echo helper ;;
        framework/*) echo "framework ${1#framework/}" ;;
        features/*/api | features/*/core | features/*/ui)
            local rest=${1#features/}
            echo "feature ${rest%%/*} ${rest#*/}"
            ;;
        *) echo "unknown $1" ;;
    esac
}

in_list() { # <word> <words...>
    local word=$1
    shift
    local each
    for each in "$@"; do
        [[ "$each" == "$word" ]] && return 0
    done
    return 1
}

rule_for() { # <half>
    case $1 in
        api) echo F1 ;;
        core) echo F2 ;;
        ui) echo F3 ;;
    esac
}

FRAMEWORK_FOR_API=(base entries/api)
FRAMEWORK_FOR_CORE=(base entries/api entries/core messages disk jobs)
FRAMEWORK_FOR_UI=(base entries/api entries/ui messages shell)
FRAMEWORK_FOR_FRONT=(base entries/api messages)

rule_break() { # <from folder> <to folder>; prints the broken rule, if any
    local -a from to
    read -r -a from <<<"$(layer_of "$1")"
    read -r -a to <<<"$(layer_of "$2")"
    if [[ ${from[0]} == unknown || ${to[0]} == unknown ]]; then
        echo "rule C1: every crate sits in app/, framework/, features/<name>/{api,core,ui}, fronts/, helpers/ or legacy/."
        return
    fi
    if [[ ${to[0]} == legacy && ${from[0]} != app && ${from[0]} != legacy ]]; then
        echo "rule L1: only app may use legacy/. Move what you need out of legacy/ first."
        return
    fi
    case ${from[0]} in
        app | legacy) return ;;
        framework)
            if [[ ${to[0]} != framework && ${to[0]} != helper ]]; then
                echo "rule W1: the framework never uses a feature or a front. Add a slot or a port instead."
            fi
            ;;
        helper)
            if [[ ${to[0]} != helper && "${to[*]}" != "framework base" ]]; then
                echo "rule H1: a helper uses only other helpers and framework/base."
            fi
            ;;
        front)
            if [[ ${to[0]} == framework ]] && in_list "${to[1]}" "${FRAMEWORK_FOR_FRONT[@]}"; then
                return
            fi
            if [[ ${to[0]} == feature && ${to[2]} == api ]]; then
                return
            fi
            echo "rule W2: a front uses only framework/{base,entries/api,messages} and feature api crates."
            ;;
        feature)
            if [[ ${to[0]} == feature && ${to[2]} != api && ${to[1]} != "${from[1]}" ]]; then
                echo "rule F4: a feature never uses another feature's ${to[2]}. Use its api, or a port in framework/jobs."
                return
            fi
            local -a framework_ok
            case ${from[2]} in
                api) framework_ok=("${FRAMEWORK_FOR_API[@]}") ;;
                core) framework_ok=("${FRAMEWORK_FOR_CORE[@]}") ;;
                ui) framework_ok=("${FRAMEWORK_FOR_UI[@]}") ;;
            esac
            case ${to[0]} in
                framework)
                    in_list "${to[1]}" "${framework_ok[@]}" && return
                    echo "rule $(rule_for "${from[2]}"): a feature's ${from[2]} may use framework/{${framework_ok[*]// /,}} only."
                    ;;
                feature)
                    [[ ${to[2]} == api ]] && return
                    if [[ ${from[2]} == ui && ${to[2]} == core ]]; then
                        echo "rule F3: a feature's ui never uses a core crate. Send a message instead."
                    elif [[ ${from[2]} == api ]]; then
                        echo "rule F1: an api crate uses only framework/base, framework/entries/api and api crates."
                    elif [[ ${from[2]} == core && ${to[2]} == ui ]]; then
                        echo "rule F2: a feature's core never uses a ui crate."
                    fi
                    ;;
                helper)
                    [[ ${from[2]} == core ]] && return
                    echo "rule F2: only a feature's core may use a helper."
                    ;;
                *) echo "rule F4: a feature uses only the framework, api crates, and its own crates." ;;
            esac
            ;;
    esac
}

expect_rule() { # <from folder> <to folder> <rule, or nothing for allowed>
    local got
    got=$(rule_break "$1" "$2")
    got=${got%%:*}
    got=${got#rule }
    if [[ "$got" != "$3" ]]; then
        echo "$0: $1 using $2 should give '${3:-allowed}', got '${got:-allowed}'." >&2
        exit 1
    fi
}

expect_rule features/places/core features/drives/core F4
expect_rule features/places/ui features/drives/ui F4
expect_rule features/places/ui features/drives/core F4
expect_rule features/places/ui features/drives/api ""
expect_rule features/places/ui features/places/core F3
expect_rule features/places/core features/places/ui F2
expect_rule features/places/core framework/shell F2
expect_rule features/places/ui framework/shell ""
expect_rule framework/shell features/places/api W1
expect_rule helpers/index legacy/ui L1

metadata=$(cargo metadata --no-deps --offline --format-version 1)
workspace_names=$(jq -r '.packages[].name' <<<"$metadata")
declare -A folder_of=()
while IFS=$'\t' read -r name manifest; do
    folder=${manifest%/Cargo.toml}
    folder_of[$name]=${folder#"$root"/}
done < <(jq -r '.packages[] | [.name, .manifest_path] | @tsv' <<<"$metadata")

if [[ ${#folder_of[@]} -eq 0 ]]; then
    echo "$0: cargo metadata listed no crates; run it from the repo root." >&2
    exit 1
fi

status=0
while IFS=$'\t' read -r name manifest deps; do
    outside=$(allowed_list "$manifest" outside)
    inside=$(allowed_list "$manifest" workspace)
    if [[ "$outside" == *MISSING* || "$inside" == *MISSING* ]]; then
        echo "$manifest: needs '# Allowed outside crates:' and '# Allowed workspace crates:' lines"
        status=1
        continue
    fi
    if [[ "$outside" == *BAD* || "$inside" == *BAD* ]]; then
        echo "$manifest: list allowed crates one per line as '# - name' under the header, or write 'none' on it"
        status=1
        continue
    fi
    for dep in $deps; do
        if grep -qxF "$dep" <<<"$workspace_names"; then
            allowed=$inside
            kind=workspace
            broken=$(rule_break "${folder_of[$name]}" "${folder_of[$dep]}")
            if [[ -n "$broken" ]]; then
                echo "$name (${folder_of[$name]}) depends on $dep (${folder_of[$dep]}): $broken"
                status=1
            fi
        else
            allowed=$outside
            kind=outside
        fi
        if ! tr -s ' ' '\n' <<<"$allowed" | grep -qxF -- "$dep"; then
            echo "$name depends on $dep, which is not in its allowed $kind crates ($manifest)."
            echo "  If it is really needed, add it to that list and say why in the PR."
            status=1
        fi
    done
done < <(jq -r '.packages[] | [.name, .manifest_path,
    ([.dependencies[] | select(.kind != "dev") | .name] | unique | join(" "))] | @tsv' <<<"$metadata")

if [[ -d legacy ]]; then
    left=$(find legacy -name '*.rs' -print0 | xargs -0 cat | wc -l)
    echo "Lines left in legacy/: $left"
fi
readonly BASE_GOAL=1500
base=$(find framework/base -name '*.rs' -print0 | xargs -0 cat | wc -l)
echo "Lines in framework/base: $base (goal: $BASE_GOAL)"
exit "$status"
