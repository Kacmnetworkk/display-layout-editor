#!/usr/bin/env bash
set -euo pipefail

plugin_id="local.display-layout-editor"
project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
plugin_source="$project_dir/omarchy-plugin"
config_home="${XDG_CONFIG_HOME:-$HOME/.config}"
plugins_dir="$config_home/omarchy/plugins"
destination="$plugins_dir/$plugin_id"

command -v cargo >/dev/null 2>&1 || {
  printf '%s\n' "cargo is required to build $plugin_id" >&2
  exit 1
}
command -v omarchy >/dev/null 2>&1 || {
  printf '%s\n' "omarchy is required to install $plugin_id" >&2
  exit 1
}

cargo build --manifest-path "$project_dir/Cargo.toml" --release
omarchy plugin validate "$plugin_source"

mkdir -p "$plugins_dir"
staging="$(mktemp -d "$plugins_dir/.${plugin_id}.stage.XXXXXX")"
cleanup() {
  rm -rf "$staging"
}
trap cleanup EXIT

cp -R "$plugin_source/." "$staging/"
install -m 0755 "$project_dir/target/release/displayeditor-rav" "$staging/bin/displayeditor-rav"
omarchy plugin validate "$staging"

if [[ -e "$destination" ]]; then
  backups_dir="$plugins_dir/.backups"
  mkdir -p "$backups_dir"
  backup="$backups_dir/${plugin_id}.$(date +%Y%m%d%H%M%S)"
  mv "$destination" "$backup"
  printf 'Backed up existing plugin to %s\n' "$backup"
fi

mv "$staging" "$destination"
trap - EXIT

omarchy-shell shell rescanPlugins

plugin_discovered=false
for _ in $(seq 1 50); do
  if omarchy plugin list --json | jq -e --arg id "$plugin_id" 'any(.[]; .id == $id)' >/dev/null; then
    plugin_discovered=true
    break
  fi
  sleep 0.1
done

if [[ $plugin_discovered != true ]]; then
  printf '%s\n' "Omarchy did not discover $plugin_id after rescanning" >&2
  exit 1
fi

omarchy plugin enable "$plugin_id"
printf 'Installed and enabled %s\n' "$plugin_id"
