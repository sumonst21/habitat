#!/bin/bash

# Unpack the hart file from our channel, repack it, and upload it to
# package-router

set -euo pipefail

source .expeditor/scripts/shared.sh

export HAB_AUTH_TOKEN="${ACCEPTANCE_HAB_AUTH_TOKEN}"
export HAB_BLDR_URL="${ACCEPTANCE_HAB_BLDR_URL}"

channel=$(get_release_channel)

echo "--- Channel: $channel - bldr url: $HAB_BLDR_URL"

release_version=$(get_latest_pkg_version_in_channel "hab")

install_latest_hab_binary "x86_64-linux"

${hab_binary} pkg install core/gzip
${hab_binary} pkg install core/tar
${hab_binary} pkg install core/wget
${hab_binary} pkg install core/zip

download_and_repackage_binary
echo "--- Uploading to S3"
aws --profile chef-cd s3 cp "$pkg_artifact" "s3://chef-habitat-artifacts/files/habitat/$release_version/$archive_name" --acl public-read
aws --profile chef-cd s3 cp "$pkg_artifact" "s3://chef-habitat-artifacts/files/habitat/$release_version/$archive_name.sha256sum" --acl public-read
