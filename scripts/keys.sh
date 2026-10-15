#!/bin/bash
# Keypair management: every keypair is GPG-encrypted at rest and decrypted only into a RAM disk
# Compatible with bash 3.x (macOS default)
#
# Layout (one tree per network; the network is the program_env, so Prod Staging uses mainnet keys):
#
#   ~/.config/solana/memo-token/<testnet|mainnet>/
#     authority/<name>-keypair.json.gpg   deploy_admin, memo_token_mint
#     programs/<name>-keypair.json.gpg    one per program (new programs go here too)
#     .../<name>-keypair.pub              public key (plaintext, used while locked)
#     .../<name>-keypair.json             link into the RAM disk, only while unlocked
#
# Each file is encrypted with symmetric GPG (AES256). All files of a network share one passphrase,
# so it is asked once per command. Use different passphrases for testnet and mainnet.
#
# Commands:
#   unlock <env>                            Decrypt all keys of <env> into the RAM disk and link them
#   lock [env]                              Remove links and decrypted keys (all networks by default);
#                                           eject the RAM disk when no network is unlocked
#   status                                  Show keys, their state, and stray plaintext keypair files
#   import <env> <authority|programs> <file> [name]
#                                           Encrypt an existing keypair file (the file is not removed)
#   new <env> <authority|programs> <name>   Generate a keypair in the RAM disk and encrypt it
#   migrate                                 Encrypt keypairs from the old plaintext layout
#                                           (authority/, programs/<env>/, target/deploy/)
#
# Run it in a terminal: the passphrase is read from the terminal, never from arguments or files.

set -e
shopt -s nullglob

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

KEYS_BASE="${HOME}/.config/solana/memo-token"
CATEGORIES="authority programs"

RAMDISK_NAME="MemoTokenKeys"
RAMDISK_MOUNT="/Volumes/${RAMDISK_NAME}"
RAMDISK_SECTORS=4096 # 512-byte sectors: 2 MiB

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

print_info() { echo -e "${BLUE}ℹ️  $1${NC}"; }
print_success() { echo -e "${GREEN}✅ $1${NC}"; }
print_warning() { echo -e "${YELLOW}⚠️  $1${NC}"; }
print_error() { echo -e "${RED}❌ $1${NC}"; }

die() {
    print_error "$1"
    exit 1
}

usage() {
    sed -n '/^# Commands:/,/^# Run it/p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
    exit 1
}

check_env() {
    case "$1" in
        testnet|mainnet) ;;
        *) die "Invalid network '$1'. Use 'testnet' or 'mainnet'" ;;
    esac
}

check_category() {
    case "$1" in
        authority|programs) ;;
        *) die "Invalid category '$1'. Use 'authority' or 'programs'" ;;
    esac
}

# ---------------------------------------------------------------------------
# Passphrase and GPG
# ---------------------------------------------------------------------------

PASSPHRASE=""

require_tty() {
    if ! { : </dev/tty; } 2>/dev/null; then
        die "No terminal to read the passphrase from. Run this command in your own terminal."
    fi
}

# Called in $(...): prints only the secret
read_secret() {
    local prompt=$1
    local value
    read -r -s -p "${prompt}" value </dev/tty
    echo >/dev/tty
    printf '%s' "${value}"
}

gpg_decrypt() {
    # $1 = encrypted file, $2 = output file ("-" for stdout)
    printf '%s' "${PASSPHRASE}" | gpg --batch --quiet --yes --pinentry-mode loopback --no-symkey-cache \
        --passphrase-fd 0 --decrypt --output "$2" "$1"
}

gpg_encrypt() {
    # $1 = plaintext file, $2 = encrypted output file
    (umask 077 && printf '%s' "${PASSPHRASE}" | gpg --batch --quiet --yes --pinentry-mode loopback --no-symkey-cache \
        --passphrase-fd 0 --symmetric --cipher-algo AES256 --output "$2" "$1")
}

encrypted_files() {
    local env=$1 category
    for category in ${CATEGORIES}; do
        for f in "${KEYS_BASE}/${env}/${category}"/*-keypair.json.gpg; do
            echo "${f}"
        done
    done
}

# Ask for the network's passphrase. With existing keys it must decrypt them; otherwise it is set
# here and asked twice.
ask_passphrase() {
    local env=$1
    local sample=$(encrypted_files "${env}" | head -n 1)
    require_tty
    if [ -n "${sample}" ]; then
        PASSPHRASE=$(read_secret "Passphrase for ${env} keys: ")
        [ -n "${PASSPHRASE}" ] || die "Empty passphrase (nothing was read from the terminal)"
        local gpg_error
        # Decrypt to stdout and discard it: gpg 2.5 writes files via "<name>.part", which fails for /dev/null
        if ! gpg_error=$(gpg_decrypt "${sample}" - 2>&1 >/dev/null); then
            if echo "${gpg_error}" | grep -q "Bad session key"; then
                die "Wrong passphrase for ${env}"
            fi
            die "GPG could not decrypt ${sample}:
${gpg_error}"
        fi
    else
        print_info "No ${env} keys yet: choose the ${env} passphrase (use a different one for each network)"
        PASSPHRASE=$(read_secret "New passphrase for ${env} keys: ")
        local confirm=$(read_secret "Repeat passphrase: ")
        [ -n "${PASSPHRASE}" ] || die "Empty passphrase"
        [ "${PASSPHRASE}" = "${confirm}" ] || die "Passphrases do not match"
    fi
}

# ---------------------------------------------------------------------------
# RAM disk
# ---------------------------------------------------------------------------

ramdisk_device() {
    diskutil info "${RAMDISK_MOUNT}" 2>/dev/null | awk '/Device Node:/ {print $3}'
}

ensure_ramdisk() {
    if [ -n "$(ramdisk_device)" ]; then
        return
    fi
    local device=$(hdiutil attach -nomount "ram://${RAMDISK_SECTORS}" | awk '{print $1}')
    diskutil erasevolume HFS+ "${RAMDISK_NAME}" "${device}" >/dev/null
    touch "${RAMDISK_MOUNT}/.metadata_never_index" # keep Spotlight out
    chmod 700 "${RAMDISK_MOUNT}"
    print_info "RAM disk mounted at ${RAMDISK_MOUNT} (${device})"
}

eject_ramdisk_if_unused() {
    local device=$(ramdisk_device)
    [ -n "${device}" ] || return 0
    if [ -d "${RAMDISK_MOUNT}/testnet" ] || [ -d "${RAMDISK_MOUNT}/mainnet" ]; then
        return 0
    fi
    hdiutil detach "${device}" >/dev/null 2>&1 || hdiutil detach -force "${device}" >/dev/null
    print_success "RAM disk ejected (${device})"
}

is_unlocked() {
    [ -d "${RAMDISK_MOUNT}/$1" ]
}

# ---------------------------------------------------------------------------
# Expected public keys (from the code and Anchor.toml), used to identify old plaintext files
# ---------------------------------------------------------------------------

code_constant() {
    # $1 = constant name, $2 = env; read from memo-mint
    local pattern='#\[cfg(not(feature = "mainnet"))\]'
    [ "$2" = "mainnet" ] && pattern='#\[cfg(feature = "mainnet")\]'
    grep -A1 "${pattern}" "${PROJECT_ROOT}/programs/memo-mint/src/lib.rs" \
        | grep "$1" | sed -E 's/.*pubkey!\("([^"]+)"\).*/\1/' | head -n 1
}

anchor_section() {
    # Values of an Anchor.toml section, as "name pubkey" lines
    awk -v section="[$1]" '
        $0 == section { inside = 1; next }
        /^\[/ { inside = 0 }
        inside && /=/ { gsub(/"/, "", $3); print $1, $3 }
    ' "${PROJECT_ROOT}/Anchor.toml"
}

# Prints "<env> <category> <name>" for a known public key, nothing otherwise
identify_pubkey() {
    local pubkey=$1 env name id
    for env in testnet mainnet; do
        if [ "${pubkey}" = "$(code_constant AUTHORIZED_ADMIN_PUBKEY "${env}")" ]; then
            echo "${env} authority deploy_admin"; return
        fi
        if [ "${pubkey}" = "$(code_constant AUTHORIZED_MINT_PUBKEY "${env}")" ]; then
            echo "${env} authority memo_token_mint"; return
        fi
        while read -r name id; do
            if [ "${pubkey}" = "${id}" ]; then
                echo "${env} programs ${name}"; return
            fi
        done <<EOF
$(anchor_section "programs.${env}")
EOF
    done
}

# ---------------------------------------------------------------------------
# Store helpers
# ---------------------------------------------------------------------------

# Encrypt a plaintext keypair into the store and verify the round trip. PASSPHRASE must be set.
store_key() {
    local env=$1 category=$2 name=$3 source=$4
    local dir="${KEYS_BASE}/${env}/${category}"
    local encrypted="${dir}/${name}-keypair.json.gpg"
    local pubkey=$(solana-keygen pubkey "${source}")

    mkdir -p "${dir}"
    chmod 700 "${KEYS_BASE}" "${KEYS_BASE}/${env}" "${dir}"
    gpg_encrypt "${source}" "${encrypted}"
    if ! gpg_decrypt "${encrypted}" - 2>/dev/null | cmp -s - "${source}"; then
        rm -f "${encrypted}"
        die "Round-trip check failed for ${encrypted}"
    fi
    echo "${pubkey}" > "${dir}/${name}-keypair.pub"
    chmod 644 "${dir}/${name}-keypair.pub"

    # Make it available right away when the network is unlocked
    if is_unlocked "${env}"; then
        local plain="${RAMDISK_MOUNT}/${env}/${category}/${name}-keypair.json"
        mkdir -p "${RAMDISK_MOUNT}/${env}/${category}"
        (umask 077 && cp "${source}" "${plain}")
        ln -sfn "${plain}" "${dir}/${name}-keypair.json"
    fi
    print_success "${env}/${category}/${name}: ${pubkey}"
}

# ---------------------------------------------------------------------------
# Commands
# ---------------------------------------------------------------------------

UNLOCK_ENV=""
UNLOCK_DONE=0

unlock_cleanup() {
    if [ -n "${UNLOCK_ENV}" ] && [ "${UNLOCK_DONE}" != 1 ]; then
        print_error "Unlock failed, cleaning up"
        remove_links "${UNLOCK_ENV}"
        rm -rf "${RAMDISK_MOUNT:?}/${UNLOCK_ENV}"
        eject_ramdisk_if_unused
    fi
}

remove_links() {
    local env=$1 category link
    for category in ${CATEGORIES}; do
        for link in "${KEYS_BASE}/${env}/${category}"/*-keypair.json; do
            if [ -L "${link}" ]; then
                rm "${link}"
            fi
        done
    done
}

cmd_unlock() {
    local env=$1
    check_env "${env}"
    is_unlocked "${env}" && die "${env} is already unlocked. Run '$0 lock ${env}' first."

    local files=$(encrypted_files "${env}")
    [ -n "${files}" ] || die "No encrypted ${env} keys in ${KEYS_BASE}/${env}. Run './scripts/setup-keypairs.sh ${env}' or '$0 migrate'."

    # Refuse if a plaintext keypair sits where a link goes
    local f link
    for f in ${files}; do
        link="${f%.gpg}"
        if [ -f "${link}" ] && [ ! -L "${link}" ]; then
            die "Plaintext keypair at ${link}. Remove it (an encrypted copy exists) and retry."
        fi
    done

    ask_passphrase "${env}"

    UNLOCK_ENV="${env}"
    trap unlock_cleanup EXIT
    ensure_ramdisk
    local category
    for category in ${CATEGORIES}; do
        (umask 077 && mkdir -p "${RAMDISK_MOUNT}/${env}/${category}")
    done

    local plain pub expected actual name
    for f in ${files}; do
        category=$(basename "$(dirname "${f}")")
        name=$(basename "${f%.gpg}")
        plain="${RAMDISK_MOUNT}/${env}/${category}/${name}"
        link="${f%.gpg}"
        (umask 077 && gpg_decrypt "${f}" "${plain}")
        actual=$(solana-keygen pubkey "${plain}")
        pub="${f%.json.gpg}.pub"
        if [ -f "${pub}" ]; then
            expected=$(cat "${pub}")
            [ "${actual}" = "${expected}" ] || die "${f} decrypts to ${actual}, but ${pub} says ${expected}"
        else
            echo "${actual}" > "${pub}"
        fi
        rm -f "${link}" # a dangling link from an earlier session
        ln -s "${plain}" "${link}"
        echo "   ${category}/${name%-keypair.json}: ${actual}"
    done
    PASSPHRASE=""
    UNLOCK_DONE=1
    print_success "${env} unlocked"
    print_warning "Run '$0 lock' when you are done."
}

cmd_lock() {
    local envs="${1:-testnet mainnet}"
    local env
    for env in ${envs}; do
        check_env "${env}"
        remove_links "${env}"
        if is_unlocked "${env}"; then
            rm -rf "${RAMDISK_MOUNT:?}/${env}"
            print_success "${env} locked"
        fi
    done
    eject_ramdisk_if_unused
    gpgconf --reload gpg-agent >/dev/null 2>&1 || true
}

cmd_status() {
    local env category pub name state
    for env in testnet mainnet; do
        echo ""
        if is_unlocked "${env}"; then
            print_warning "${env}: UNLOCKED"
        else
            print_success "${env}: locked"
        fi
        for category in ${CATEGORIES}; do
            for pub in "${KEYS_BASE}/${env}/${category}"/*-keypair.pub; do
                name=$(basename "${pub%-keypair.pub}")
                local link="${pub%.pub}.json"
                if [ ! -f "${pub%.pub}.json.gpg" ]; then
                    state="MISSING ENCRYPTED FILE"
                elif [ -L "${link}" ] && [ -f "${link}" ]; then
                    state="unlocked"
                elif [ -f "${link}" ]; then
                    state="PLAINTEXT ON DISK"
                else
                    state="locked"
                fi
                printf "   %-10s %-16s %-45s %s\n" "${category}" "${name}" "$(cat "${pub}")" "${state}"
            done
        done
    done

    # Plaintext keypairs anywhere under the key directory, plus real keys in target/deploy.
    # cargo build-sbf generates a random <program>-keypair.json in target/deploy when none is there;
    # those match no known key, are never used, and are only listed.
    local stray=$(find "${KEYS_BASE}" -type f -name '*.json' 2>/dev/null | sort)
    local random_build="" f id
    for f in "${PROJECT_ROOT}/target/deploy"/*-keypair.json; do
        [ -f "${f}" ] && [ ! -L "${f}" ] || continue
        id=$(identify_pubkey "$(solana-keygen pubkey "${f}" 2>/dev/null)")
        if [ -n "${id}" ]; then
            stray="${stray}${stray:+
}${f}"
        else
            random_build="${random_build} $(basename "${f}")"
        fi
    done
    echo ""
    if [ -n "${stray}" ]; then
        print_warning "Plaintext keypair files on disk:"
        echo "${stray}" | while read -r f; do
            id=$(identify_pubkey "$(solana-keygen pubkey "${f}" 2>/dev/null)")
            echo "   ${f}${id:+  (${id})}"
        done
        echo "   Encrypt them ('$0 migrate' or '$0 import'), keep plaintext only offline, then remove them."
    else
        print_success "No plaintext keypair files on disk"
    fi
    if [ -n "${random_build}" ]; then
        print_info "target/deploy has random keypairs generated by the build (not program IDs, never used):${random_build}"
    fi
}

cmd_import() {
    local env=$1 category=$2 source=$3 name=$4
    check_env "${env}"
    check_category "${category}"
    [ -f "${source}" ] || die "File not found: ${source}"
    if [ -z "${name}" ]; then
        name=$(basename "${source}")
        name="${name%.json}"
        name="${name%-keypair}"
    fi
    [ ! -e "${KEYS_BASE}/${env}/${category}/${name}-keypair.json.gpg" ] \
        || die "${env}/${category}/${name} already exists"
    solana-keygen pubkey "${source}" >/dev/null 2>&1 || die "${source} is not a keypair file"

    ask_passphrase "${env}"
    store_key "${env}" "${category}" "${name}" "${source}"
    PASSPHRASE=""
    print_info "${source} was not removed. Keep plaintext copies only in offline storage."
}

cmd_new() {
    local env=$1 category=$2 name=$3
    check_env "${env}"
    check_category "${category}"
    [ -n "${name}" ] || die "Usage: $0 new <env> <authority|programs> <name>"
    [ ! -e "${KEYS_BASE}/${env}/${category}/${name}-keypair.json.gpg" ] \
        || die "${env}/${category}/${name} already exists"

    ask_passphrase "${env}"
    ensure_ramdisk
    local tmp="${RAMDISK_MOUNT}/.new-$$"
    (umask 077 && mkdir -p "${tmp}")
    trap 'rm -rf "${tmp}"; eject_ramdisk_if_unused' EXIT
    solana-keygen new --no-bip39-passphrase --silent --outfile "${tmp}/key.json" >/dev/null
    store_key "${env}" "${category}" "${name}" "${tmp}/key.json"
    PASSPHRASE=""
    print_info "Back up ${KEYS_BASE}/${env}/${category}/${name}-keypair.json.gpg to offline storage."
}

cmd_migrate() {
    # Candidate plaintext files from the old layouts
    local candidates=$( (for f in "${KEYS_BASE}/authority"/*.json "${KEYS_BASE}/programs/testnet"/*.json \
                                  "${KEYS_BASE}/programs/mainnet"/*.json "${PROJECT_ROOT}/target/deploy"/*-keypair.json; do
                            [ -f "${f}" ] && [ ! -L "${f}" ] && echo "${f}"
                         done) | sort)
    if [ -z "${candidates}" ]; then
        print_success "No plaintext keypairs in the old layout"
    fi

    local env f pubkey id category name migrated="" pending
    for env in testnet mainnet; do
        pending=""
        for f in ${candidates}; do
            pubkey=$(solana-keygen pubkey "${f}" 2>/dev/null) || continue
            id=$(identify_pubkey "${pubkey}")
            [ "${id%% *}" = "${env}" ] && pending="${pending} ${f}"
        done
        [ -n "${pending}" ] || continue

        echo ""
        print_info "Migrating ${env} keys"
        ask_passphrase "${env}"
        for f in ${pending}; do
            pubkey=$(solana-keygen pubkey "${f}")
            id=$(identify_pubkey "${pubkey}")
            category=$(echo "${id}" | awk '{print $2}')
            name=$(echo "${id}" | awk '{print $3}')
            local pub="${KEYS_BASE}/${env}/${category}/${name}-keypair.pub"
            if [ -f "${pub%.pub}.json.gpg" ]; then
                if [ "$(cat "${pub}" 2>/dev/null)" = "${pubkey}" ]; then
                    print_info "${env}/${category}/${name}: already encrypted (${f})"
                    migrated="${migrated} ${f}"
                else
                    print_error "${env}/${category}/${name}: encrypted key differs from ${f}; skipped"
                fi
                continue
            fi
            store_key "${env}" "${category}" "${name}" "${f}"
            migrated="${migrated} ${f}"
        done
        PASSPHRASE=""
    done

    # Files that match no known key are left alone
    for f in ${candidates}; do
        pubkey=$(solana-keygen pubkey "${f}" 2>/dev/null) || pubkey="?"
        [ -n "$(identify_pubkey "${pubkey}")" ] || print_warning "Unknown key ${pubkey}, not migrated: ${f}"
    done

    # Old links into the previous layout (cargo build-sbf would write a new keypair through a dangling link)
    for f in "${PROJECT_ROOT}/target/deploy"/*-keypair.json; do
        if [ -L "${f}" ]; then
            rm "${f}"
            print_info "Removed old link ${f}"
        fi
    done

    [ -n "${migrated}" ] || return 0
    echo ""
    print_warning "These plaintext files now have verified encrypted copies:"
    for f in ${migrated}; do
        echo "   ${f}"
    done
    echo "Make sure an offline backup exists before removing them."
    local answer
    read -r -p "Remove these plaintext files now? (yes/no): " answer </dev/tty
    if [ "${answer}" = "yes" ]; then
        for f in ${migrated}; do
            rm -P "${f}" 2>/dev/null || rm -f "${f}"
        done
        rmdir "${KEYS_BASE}/authority" "${KEYS_BASE}/programs/testnet" "${KEYS_BASE}/programs/mainnet" \
              "${KEYS_BASE}/programs" 2>/dev/null || true
        print_success "Plaintext files removed"
    else
        print_info "Kept. Remove them later; '$0 status' lists plaintext keypair files."
    fi
}

case "$1" in
    unlock) [ -n "$2" ] || usage; cmd_unlock "$2" ;;
    lock) cmd_lock "$2" ;;
    status) cmd_status ;;
    import) [ -n "$4" ] || usage; cmd_import "$2" "$3" "$4" "$5" ;;
    new) [ -n "$4" ] || usage; cmd_new "$2" "$3" "$4" ;;
    migrate) cmd_migrate ;;
    *) usage ;;
esac
