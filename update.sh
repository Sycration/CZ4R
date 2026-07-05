#!/usr/bin/env sh
# This script is used to update the version of the project in Cargo.toml and README.md
# Commit the changes and push them to the repository
# Builds the docker image and pushes it to Docker Hub
# Usage: ./update.sh <new_version_string>

set -e

DOCKER_REPO="sycration/cz4r"

if [ -z "$1" ]; then
    echo "Usage: ./update.sh <new_version_string>"
    exit 1
fi

NEW_VERSION="$1"

echo "Updating version to $NEW_VERSION..."

# Update version in Cargo.toml
sed -i.bak -E "s|^version = \".*\"|version = \"$NEW_VERSION\"|" Cargo.toml
rm -f Cargo.toml.bak

# Update version in README.md
sed -i.bak -E "s|$DOCKER_REPO:[0-9]+\.[0-9]+\.[0-9]+|$DOCKER_REPO:$NEW_VERSION|g" README.md
rm -f README.md.bak

# Update Cargo.lock to reflect the new version
cargo update -p cz4r --precise "$NEW_VERSION" 2>/dev/null || true

# Commit and push the changes
git add Cargo.toml Cargo.lock README.md
git commit -m "Bump version to $NEW_VERSION"
git push

# Build and push the docker image
sudo docker build -t "$DOCKER_REPO:$NEW_VERSION" -t "$DOCKER_REPO:latest" .
sudo docker push "$DOCKER_REPO:$NEW_VERSION"
sudo docker push "$DOCKER_REPO:latest"

echo "Successfully updated to version $NEW_VERSION and pushed docker image $DOCKER_REPO:$NEW_VERSION"
