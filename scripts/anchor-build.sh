#!/bin/bash
# Guarded `anchor build`, sourced by the deploy, local-validator and regression scripts (bash 3.x compatible)
#
# anchor-cli 0.32.1 run in a tree without target/deploy generates random program keypairs and
# rewrites the first declare_id! of every programs/*/src/lib.rs to match them. That is the
# #[cfg(feature = "mainnet")] one, so testnet builds look normal while a --features mainnet build
# carries random program IDs (MEMO-MINT-AUDIT.md, MINT-22). The same key sync also rewrites the
# program addresses in Anchor.toml, re-serializing it without the sections Anchor does not know.
#
# anchor_build [anchor build arguments...]
#   Runs from the project root. Creates target/deploy first (an existing directory, even empty,
#   prevents the rewrite). If the build still changes any programs/*/src/{lib,id}.rs or Anchor.toml,
#   restores the files and returns 1. An interrupted build (Ctrl-C, SIGTERM) restores them too.

# Files the key sync can rewrite, relative to the project root
_anchor_build_guarded_files() {
    local root=$1
    local file
    for file in "${root}"/Anchor.toml "${root}"/programs/*/src/lib.rs "${root}"/programs/*/src/id.rs; do
        [ -f "${file}" ] && echo "${file#${root}/}"
    done
}

# Restores every guarded file that differs from its saved copy; prints the restored paths
_anchor_build_restore() {
    local root=$1
    local backup=$2
    local file
    for file in $(_anchor_build_guarded_files "${root}"); do
        if ! cmp -s "${root}/${file}" "${backup}/${file}"; then
            cp "${backup}/${file}" "${root}/${file}"
            echo "${file}"
        fi
    done
}

anchor_build() {
    local root
    root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
    mkdir -p "${root}/target/deploy"

    local backup
    backup="$(mktemp -d)"
    local file
    for file in $(_anchor_build_guarded_files "${root}"); do
        mkdir -p "${backup}/$(dirname "${file}")"
        cp "${root}/${file}" "${backup}/${file}"
    done

    # The build is the slow part: restore the sources if it is interrupted
    trap "_anchor_build_restore '${root}' '${backup}' >/dev/null; rm -rf '${backup}'; trap - INT TERM; exit 130" INT TERM

    local status=0
    (cd "${root}" && anchor build "$@") || status=$?

    trap - INT TERM
    local changed
    changed="$(_anchor_build_restore "${root}" "${backup}")"
    rm -rf "${backup}"

    if [ -n "${changed}" ]; then
        echo "❌ anchor build rewrote program IDs (declare_id! / Anchor.toml); restored these files; do not use this build output:" >&2
        echo "${changed}" | sed 's/^/   /' >&2
        echo "   Check target/deploy and the program IDs before building again (MEMO-MINT-AUDIT.md, MINT-22)" >&2
        return 1
    fi
    return ${status}
}
