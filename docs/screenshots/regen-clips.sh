#!/usr/bin/env bash
# Regenerates the short videos of the user guide (docs/userguide/z_media/clips/) from clips.conf (DEV-019):
# all of them, or the names given; the same as regen.sh --clips (see there for the environment).
exec "$(dirname "${BASH_SOURCE[0]}")/regen.sh" --clips "$@"
