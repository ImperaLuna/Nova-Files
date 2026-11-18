# Sourced by the tools/ checks: the folders that hold the crates.
CODE_DIRS=()
for dir in app framework features fronts helpers legacy; do
    if [[ -d $dir ]]; then
        CODE_DIRS+=("$dir")
    fi
done
if [[ ${#CODE_DIRS[@]} -eq 0 || -z "$(find "${CODE_DIRS[@]}" -name '*.rs' -print -quit)" ]]; then
    echo "$0: found no Rust files in app framework features fronts helpers legacy; run it from the repo root." >&2
    exit 1
fi
