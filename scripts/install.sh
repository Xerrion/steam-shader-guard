#!/bin/sh
set -eu

fail() {
    printf 'Steam Shader Guard installer: %s\n' "$*" >&2
    exit 1
}

for tool in id uname curl mktemp awk sha256sum chmod rm rmdir; do
    command -v "$tool" >/dev/null 2>&1 ||
        fail "A required command is missing: $tool. Install it and try again."
done

uid=$(id -u) || fail "Could not check your user account. Nothing was installed."
[ "$uid" != 0 ] ||
    fail "Use your normal login account. Do not run this installer as root or with sudo."
system=$(uname -s) || fail "Could not check your operating system. Nothing was installed."
machine=$(uname -m) || fail "Could not check your processor type. Nothing was installed."
[ "$system:$machine" = Linux:x86_64 ] ||
    fail "This installer only supports Linux on 64-bit Intel or AMD PCs (x86_64). Nothing was installed."

umask 077
temporary=$(mktemp -d "${TMPDIR:-/tmp}/steam-shader-guard.XXXXXXXXXX") ||
    fail "Could not create a folder for the download. Nothing was installed."
[ -n "$temporary" ] || fail "Could not find the folder for the download. Nothing was installed."
cleanup() {
    rm -f -- "$temporary/SHA256SUMS" "$temporary/steam-shader-guard"
    rmdir -- "$temporary"
}
trap cleanup 0
trap 'exit 1' HUP INT TERM

repository=https://github.com/Xerrion/steam-shader-guard
printf '%s\n' \
    "Finding the latest version of Steam Shader Guard." \
    "This installation will not change game settings, copy existing shaders, or start Steam."
# Resolve /latest once. All asset requests use the resulting release tag.
resolved=$(curl -q -fsSL --proto '=https' --proto-redir '=https' \
    -o /dev/null -w '%{url_effective}#' "$repository/releases/latest") ||
    fail "Could not find the latest version. Nothing was installed. Check your internet connection and try again."
# The suffix preserves trailing newlines so tag validation cannot discard them.
resolved=${resolved%#}
case "$resolved" in
    "$repository/releases/tag/v"*) version=${resolved#"$repository/releases/tag/v"} ;;
    *) fail "The download page did not identify a supported version. Nothing was installed." ;;
esac
printf '%s\n' "$version" | awk '
    NR != 1 || $0 !~ /^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(\+[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$/ {
        exit 1
    }
' || fail "The download page did not identify a supported version. Nothing was installed."
release="$repository/releases/download/v$version"
versioned="steam-shader-guard-$version-x86_64-unknown-linux-musl"

printf '%s\n' "Downloading the information needed to check the program."
curl -q -fsSL --proto '=https' --proto-redir '=https' \
    -o "$temporary/SHA256SUMS" "$release/SHA256SUMS" ||
    fail "Could not download the information needed to check the program. Nothing was installed. Please try again."
# Require sha256sum format and exactly one supported executable entry.
# Do not select archives, path-prefixed names, or multiple executable candidates.
entry=$(awk -v versioned="$versioned" '
    {
        digest = substr($0, 1, 64)
        marker = substr($0, 65, 2)
        name = substr($0, 67)
        if (length(digest) != 64 || digest !~ /^[0-9A-Fa-f]+$/ ||
            (marker != "  " && marker != " *") ||
            name == "" || name ~ /[[:cntrl:]]/ || seen[name]++) {
            invalid = 1
            next
        }
        if (name == versioned || name == "steam-shader-guard") {
            count++
            selected = digest " " name
        }
    }
    END {
        if (invalid || count != 1) exit 1
        print selected
    }
' "$temporary/SHA256SUMS") ||
    fail "The download information is missing or unclear, so the installer cannot check the program. Nothing was installed."
digest=${entry%% *}
asset=${entry#* }

printf 'Downloading Steam Shader Guard version %s.\n' "$version"
curl -q -fsSL --proto '=https' --proto-redir '=https' \
    -o "$temporary/steam-shader-guard" "$release/$asset" ||
    fail "Could not download Steam Shader Guard. Nothing was installed. Check your internet connection and try again."
printf '%s\n' "Checking that the downloaded program matches the published copy before running it."
(
    cd "$temporary" || exit 1
    printf '%s  steam-shader-guard\n' "$digest" | sha256sum --check - >/dev/null
) || fail "The download did not pass the check. Nothing was installed. Please try again."

chmod 700 "$temporary/steam-shader-guard" ||
    fail "Could not prepare the downloaded program to run. Nothing was installed."
printf '%s\n' "Installing Steam Shader Guard and adding 'Steam (Shader Guard)' to your application menu."
if "$temporary/steam-shader-guard" install --apply; then
    printf '%s\n' \
        "Installation complete." \
        "The installer did not change game settings, copy existing shaders, or start Steam." \
        "Next steps:" \
        "1. Check Steam and find your game's ID. Copy this command into your terminal:" \
        "   ~/.local/bin/steam-shader-guard doctor" \
        "Replace GAME_ID with the game's number from doctor. Do not type GAME_ID literally." \
        "Fully exit Steam and any running games before steps 2 and 3." \
        "2. Optional: copy your game's existing shaders before connecting it." \
        "   ~/.local/bin/steam-shader-guard recover GAME_ID --apply" \
        "3. When you are ready, connect that game to Shader Guard." \
        "   ~/.local/bin/steam-shader-guard enable GAME_ID --apply" \
        "4. Start 'Steam (Shader Guard)' from your application menu, then play normally."
else
    exit_code=$?
    fail "Installation failed (error code $exit_code). Read the error above before trying again."
fi
