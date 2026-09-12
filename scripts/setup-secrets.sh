#!/usr/bin/env bash
# Interactively sets the GitHub Actions secrets the workflows need, one at a time, with `gh`.
#
#   scripts/setup-secrets.sh
#
# Secrets:
#   CARGO_REGISTRY_TOKEN      crates.io API token (publish scope) — set on avdump-rust and mediainfo-rust
#   MEDIAINFO_RUST_DEPLOY_KEY private SSH key with read access to mediainfo-rust — set on avdump-rust;
#                             the script can generate a key pair and register the public half as a
#                             deploy key on the mediainfo-rust repository for you.
# Requires: gh (logged in with repo scope), ssh-keygen.
set -euo pipefail

AVDUMP_REPO=${AVDUMP_REPO:-sandwichfarm/avdump-rust}
MEDIAINFO_REPO=${MEDIAINFO_REPO:-sandwichfarm/mediainfo-rust}

say() { printf '\n\033[1m%s\033[0m\n' "$*"; }
ask_yn() { local a; read -r -p "$1 [y/N] " a; [[ ${a,,} == y* ]]; }

command -v gh >/dev/null || { echo "gh is not installed" >&2; exit 1; }
gh auth status >/dev/null 2>&1 || { echo "gh is not logged in: run 'gh auth login'" >&2; exit 1; }

set_secret() { # name repo  (value on stdin)
  gh secret set "$1" --repo "$2" && echo "  set $1 on $2"
}

# ---------------------------------------------------------------- CARGO_REGISTRY_TOKEN
say "1/2  CARGO_REGISTRY_TOKEN (crates.io API token with the 'publish-new' and 'publish-update' scopes)"
echo "     Create one at https://crates.io/settings/tokens. Leave empty to skip."
read -r -s -p "     Token: " token; echo
if [[ -n $token ]]; then
  for repo in "$AVDUMP_REPO" "$MEDIAINFO_REPO"; do
    if ask_yn "     Set on $repo?"; then
      printf '%s' "$token" | set_secret CARGO_REGISTRY_TOKEN "$repo"
    fi
  done
else
  echo "     skipped"
fi
unset token

# ---------------------------------------------------------------- MEDIAINFO_RUST_DEPLOY_KEY
say "2/2  MEDIAINFO_RUST_DEPLOY_KEY (SSH key that can read $MEDIAINFO_REPO; used by $AVDUMP_REPO CI/Docker)"
echo "     Only needed while $MEDIAINFO_REPO is private."
echo "     [g] generate a new ed25519 key pair and add the public key as a read-only deploy key"
echo "     [p] paste an existing private key"
echo "     [s] skip"
read -r -p "     Choice [g/p/s]: " choice
case ${choice,,} in
  g)
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT
    ssh-keygen -q -t ed25519 -N '' -C "avdump-rust CI deploy key" -f "$tmp/key"
    gh repo deploy-key add "$tmp/key.pub" --repo "$MEDIAINFO_REPO" --title "avdump-rust CI (read-only)" \
      && echo "  added deploy key to $MEDIAINFO_REPO"
    set_secret MEDIAINFO_RUST_DEPLOY_KEY "$AVDUMP_REPO" < "$tmp/key"
    ;;
  p)
    echo "     Paste the private key (PEM), then press Ctrl-D on an empty line:"
    key=$(cat)
    if [[ -n $key ]]; then
      printf '%s\n' "$key" | set_secret MEDIAINFO_RUST_DEPLOY_KEY "$AVDUMP_REPO"
    else
      echo "     empty, skipped"
    fi
    unset key
    ;;
  *) echo "     skipped" ;;
esac

say "Done. Current secrets:"
for repo in "$AVDUMP_REPO" "$MEDIAINFO_REPO"; do
  echo "  $repo:"; gh secret list --repo "$repo" | sed 's/^/    /'
done
