#!/usr/bin/env bash
set -euo pipefail

if [[ $# -eq 0 ]]; then
  echo "Usage: $0 <AppImage>..." >&2
  exit 2
fi

for tool in unsquashfs mksquashfs; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "$tool is required to check AppImage icons" >&2
    exit 1
  }
done

for image in "$@"; do
  [[ -f "$image" ]] || { echo "AppImage not found: $image" >&2; exit 1; }
  [[ "$image" == */* ]] || image="./$image"
  offset="$("$image" --appimage-offset)"
  [[ "$offset" =~ ^[0-9]+$ ]] || { echo "Invalid AppImage offset: $image" >&2; exit 1; }

  work_dir="$(mktemp -d)"
  trap 'rm -rf "$work_dir"' EXIT
  unsquashfs -quiet -offset "$offset" -dest "$work_dir/AppDir" "$image"

  if [[ ! -e "$work_dir/AppDir/.DirIcon" ]]; then
    icon="$(find -L "$work_dir/AppDir" -maxdepth 1 -type f -name '*.png' -print -quit)"
    [[ -n "$icon" ]] || { echo "No root PNG icon in $image" >&2; exit 1; }
    ln -sfn "$(basename "$icon")" "$work_dir/AppDir/.DirIcon"
    head -c "$offset" "$image" > "$work_dir/runtime"
    mksquashfs "$work_dir/AppDir" "$work_dir/filesystem.squashfs" -noappend -comp xz -quiet >/dev/null
    cat "$work_dir/runtime" "$work_dir/filesystem.squashfs" > "$work_dir/repaired.AppImage"
    chmod --reference="$image" "$work_dir/repaired.AppImage"
    mv -f "$work_dir/repaired.AppImage" "$image"
    echo "Added .DirIcon to $image"
  else
    echo ".DirIcon present in $image"
  fi

  unsquashfs -quiet -offset "$offset" -dest "$work_dir/verify" "$image" >/dev/null
  [[ -e "$work_dir/verify/.DirIcon" ]] || {
    echo "AppImage icon verification failed: $image" >&2
    exit 1
  }

  rm -rf "$work_dir"
  trap - EXIT
done
