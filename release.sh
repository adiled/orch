#!/usr/bin/env bash
set -euo pipefail

# release.sh - bump version, sync lockfile, tag, and push.
# Usage: ./release.sh [patch|minor|major]
#
# Examples:
#     ./release.sh patch            # bumps 0.2.6 -> 0.2.7
#     ./release.sh minor            # bumps 0.2.6 -> 0.3.0
#     ./release.sh major            # bumps 0.2.6 -> 1.0.0

BRANCH="$(git branch --show-current)"
if [[ "$BRANCH" != "main" ]]; then
  echo "error: must be on main (currently on '$BRANCH')" >&2
  exit 1
fi

# Sync with origin
git fetch origin main
git reset --hard origin/main

# Determine version
NEW_VER=""
case "${1:-patch}" in
  patch)
    OLD_VER="$(grep '^version' Cargo.toml | sed 's/.*"\(.*\)"/\1/')"
    IFS='.' read -r MAJOR MINOR PATCH <<< "$OLD_VER"
    NEW_VER="$MAJOR.$MINOR.$((PATCH + 1))"
     ;;
  minor)
    OLD_VER="$(grep '^version' Cargo.toml | sed 's/.*"\(.*\)"/\1/')"
    IFS='.' read -r MAJOR MINOR PATCH <<< "$OLD_VER"
    NEW_VER="$MAJOR.$((MINOR + 1)).0"
     ;;
  major)
    OLD_VER="$(grep '^version' Cargo.toml | sed 's/.*"\(.*\)"/\1/')"
    IFS='.' read -r MAJOR MINOR PATCH <<< "$OLD_VER"
    NEW_VER="$((MAJOR + 1)).0.0"
     ;;
  *)
    echo "error: unknown bump type '${1}' (use patch|minor|major)" >&2
    exit 1
     ;;
esac

# Bump Cargo.toml
sed -i '' "s/version = \"$OLD_VER\"/version = \"$NEW_VER\"/" Cargo.toml

# Update Cargo.lock to match
cargo generate-lockfile

# Verify no uncommitted changes aside from version bump
if ! git diff --quiet -- Cargo.toml Cargo.lock; then
  git add Cargo.toml Cargo.lock
  git commit -m "chore: bump to $NEW_VER"
fi

# Tag and push
TAG="v$NEW_VER"
git tag -a "$TAG" -m "v$NEW_VER"
git push origin main --tags
echo "Released $TAG ✓"
