#!/bin/sh
set -e

# ---------------------------------------------------------------------------
# Stakpak Agent container entrypoint  (root → gosu drop pattern)
#
# When the sandbox spawns a container with --user 0:0 and sets
# STAKPAK_TARGET_UID / STAKPAK_TARGET_GID, this script:
#   1. Patches /etc/passwd + /etc/group so tools see a valid "agent" identity
#   2. Chowns writable home-directory paths (skipping read-only bind mounts)
#   3. Drops privileges via gosu and exec's the real command
#
# When no target UID is set (direct `docker run`, macOS, or UID already
# matches), the script is a transparent pass-through.
# ---------------------------------------------------------------------------

IMAGE_UID=1000
IMAGE_GID=1000
export HOME=/home/agent

CURRENT_UID=$(id -u)

if [ "$CURRENT_UID" = "0" ] && [ -n "$STAKPAK_TARGET_UID" ]; then
    # Running as root inside the sandbox with a target UID requested.
    TARGET_UID="$STAKPAK_TARGET_UID"
    TARGET_GID="${STAKPAK_TARGET_GID:-$TARGET_UID}"

    if [ "$TARGET_UID" != "$IMAGE_UID" ] || [ "$TARGET_GID" != "$IMAGE_GID" ]; then
        sed -i "s/^agent:x:${IMAGE_UID}:${IMAGE_GID}:/agent:x:${TARGET_UID}:${TARGET_GID}:/" /etc/passwd
        sed -i "s/^agent:x:${IMAGE_GID}:/agent:x:${TARGET_GID}:/" /etc/group

        # Chown the image-owned home tree, skipping bind-mounted sub-trees
        # (which may be read-only).  -xdev prevents crossing filesystem
        # boundaries, so :ro mounts like .stakpak/config.toml, .ssh/, etc.
        # are untouched.  The predicate catches both UID and GID mismatches.
        find /home/agent -xdev \( -not -user "$TARGET_UID" -o -not -group "$TARGET_GID" \) \
            -exec chown "$TARGET_UID:$TARGET_GID" {} + 2>/dev/null || true

        # Explicitly fix writable named volumes that -xdev skips above.
        # These are Stakpak-managed caches, not user bind mounts.
        for dir in /home/agent/.local/share/aquaproj-aqua; do
            if [ -d "$dir" ]; then
                find "$dir" \( -not -user "$TARGET_UID" -o -not -group "$TARGET_GID" \) \
                    -exec chown "$TARGET_UID:$TARGET_GID" {} + 2>/dev/null || true
            fi
        done
    fi

    # Drop to the (now-remapped) agent user.
    exec gosu agent "$@"
fi

# No remapping needed — run as the current user.
exec "$@"
