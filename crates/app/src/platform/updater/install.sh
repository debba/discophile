#!/bin/sh
parent=$1 target=$2 staged=$3 backup=$4 root=$5 failure=$6 platform=$7
launch() {
    if [ "$platform" = macos ]; then
        /usr/bin/open -n "$target"
    else
        "$target" </dev/null >/dev/null 2>&1 &
        child=$!
        sleep 1
        kill -0 "$child" 2>/dev/null
    fi
}
fail() {
    printf '%s\n' "$1" > "$failure"
    exit 1
}
attempt=0
while kill -0 "$parent" 2>/dev/null; do
    attempt=$((attempt + 1))
    [ "$attempt" -lt 120 ] || fail 'Discophile did not exit. The update was not installed.'
    sleep 1
done
if ! mv "$target" "$backup"; then
    printf '%s\n' 'Cannot move the installed version. Nothing was installed.' > "$failure"
    launch
    exit 1
fi
if ! mv "$staged" "$target"; then
    mv "$backup" "$target" || fail "Cannot restore the previous version. It is preserved at: $backup"
    printf '%s\n' 'Installation failed. The previous version was restored.' > "$failure"
    launch
    exit 1
fi
if ! launch; then
    mv "$target" "$staged" && mv "$backup" "$target" || fail "Cannot restart. The previous version is preserved at: $backup"
    printf '%s\n' 'Restart failed. The previous version was restored.' > "$failure"
    launch
    exit 1
fi
rm -rf "$root"
