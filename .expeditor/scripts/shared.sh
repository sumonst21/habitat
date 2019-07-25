#!/bin/bash

set -euo pipefail

# Download public and private keys for the "core" origin from Builder.
#
# Currently relies on a global variable `hab_binary` being set, since
# in the Linux build process, we need to switch binaries mid-way
# through the pipeline. As we bring more platforms into play, this may
# change. FYI.
import_keys() {
    echo "--- :key: Downloading 'core' public keys from ${HAB_BLDR_URL}"
    ${hab_binary:?} origin key download core
    echo "--- :closed_lock_with_key: Downloading latest 'core' secret key from ${HAB_BLDR_URL}"
    ${hab_binary:?} origin key download \
        --auth="${HAB_AUTH_TOKEN}" \
        --secret \
        core
}

get_latest_pkg_version_in_channel() {
    local pkg_name="${1:?}"
    version=$(curl -s "${HAB_BLDR_URL}/v1/depot/channels/core/$(get_release_channel)/pkgs/${pkg_name}/latest?target=${BUILD_PKG_TARGET}" \
        | jq -r '.ident | .version + "/" + .release')
    echo "${version}"
}

# Always install the latest hab binary appropriate for your linux platform
#
# Accepts a pkg target argument if you need to override it, otherwise
# will default to the value of `BUILD_PKG_TARGET`
install_latest_hab_binary() {
    local pkg_target="${1:-$BUILD_PKG_TARGET}"
    echo "--- Installing latest hab binary for $pkg_target using curl|bash"
    # TODO:
    # really weird corner case on linux2 because the 0.82.0 versions of both
    # are the same. let's just delete it
    rm -rf /hab/pkgs/core/hab/0.82.0
    curl https://raw.githubusercontent.com/habitat-sh/habitat/master/components/hab/install.sh | sudo bash -s -- -t "$pkg_target"
    hab_binary="/bin/hab"
    echo "--- :habicat: Installed latest stable hab: $(${hab_binary} --version)"

    # now install the latest hab available in our channel, if it and the studio exist yet
    hab_version=$(get_latest_pkg_version_in_channel "hab" | cut -d'/' -f 1)
    studio_version=$(get_latest_pkg_version_in_channel "hab-studio" | cut -d'/' -f 1)

    if [[ -n $hab_version && -n $studio_version && $hab_version == "$studio_version" ]]; then
        echo "-- Hab and studio versions match! Found hab: ${hab_version:-null} - studio: ${studio_version:-null}. Upgrading :awesome:"
        channel=$(get_release_channel)
        ${hab_binary:?} pkg install --binlink --force --channel "${channel}" core/hab
        ${hab_binary:?} pkg install --binlink --force --channel "${channel}" core/hab-studio
        echo "--- :habicat: Installed latest build hab: $(${hab_binary} --version)"
    else
        echo "-- Hab and studio versions did not match. hab: ${hab_version:-null} - studio: ${studio_version:-null}"
    fi
    declare -g hab_binary
}

download_and_repackage_binary() {
    tmp_root="$(mktemp -d -t "grant-XXXX")"
    extract_dir="$tmp_root/extract"
    mkdir -p $extract_dir

    echo "--- Downloading $release_version for $BUILD_PKG_TARGET"
    hab pkg exec core/wget wget "${HAB_BLDR_URL}/v1/depot/pkgs/core/hab/$release_version/download?target=$BUILD_PKG_TARGET" -O "$tmp_root/hab-$channel.hart"

    target_hart="$tmp_root/hab-$channel.hart"
    tail -n+6 "${target_hart}" | \
        tar --directory "${extract_dir}" \
            --extract \
            --xz \
            --strip-components=6

    if [[ $(find "${extract_dir}" \( -name hab -or -name hab.exe \) -type f | wc -l) -ne 1 ]]; then
    exit_with "$target_hart did not contain a \`hab' binary" 2
    fi

    local extracted_hab_binary
    extracted_hab_binary="$(find "$extract_dir" \( -name hab -or -name hab.exe \) -type f)"
    pkg_target="$(tr --delete '\r' < "${extract_dir}"/TARGET)"
    pkg_arch="$(echo "$pkg_target" | cut -d '-' -f 1)"
    pkg_kernel="$(echo "$pkg_target" | cut -d '-' -f 2)"
    pkg_ident="$(tr --delete '\r' < "$extract_dir"/IDENT)"
    pkg_origin="$(echo "$pkg_ident" | cut -d '/' -f 1)"
    pkg_name="$(echo "$pkg_ident" | cut -d '/' -f 2)"
    pkg_version="$(echo "$pkg_ident" | cut -d '/' -f 3)"
    pkg_release="$(echo "$pkg_ident" | cut -d '/' -f 4)"
    local archive_name build_dir pkg_dir
    archive_name="hab-$(echo "$pkg_ident" | cut -d '/' -f 3-4 | tr '/' '-')-$pkg_target"
    build_dir="$tmp_root/build"
    pkg_dir="$build_dir/${archive_name}"

    echo "Copying $extracted_hab_binary to $(basename "$pkg_dir")"
    mkdir -p "$pkg_dir"
    mkdir -p "$tmp_root/results"

    if [[ $pkg_target == *"windows" ]]; then
    for file in "$(dirname "$extracted_hab_binary")"/*; do 
        cp -p "$file" "$pkg_dir/"
    done
    else
    cp -p "$extracted_hab_binary" "$pkg_dir/$(basename "$extracted_hab_binary")"
    fi

    echo "Compressing \`hab' binary"
    pushd "$build_dir" >/dev/null
    case "$pkg_target" in
    *-linux | *-linux-kernel2)
        pkg_artifact="$tmp_root/results/${archive_name}.tar.gz"
        local tarball
        tarball="$build_dir/$(basename "${pkg_artifact%.gz}")"
        hab pkg exec core/tar tar cf "$tarball" "$(basename "$pkg_dir")"
        rm -fv "$pkg_artifact"
        hab pkg exec core/gzip gzip -9 -c "$tarball" > "$pkg_artifact"
        ;;
    *-darwin | *-windows)
        pkg_artifact="$tmp_root/results/${archive_name}.zip"
        rm -fv "$pkg_artifact"
        hab pkg exec core/zip zip -9 -r "$pkg_artifact" "$(basename "$pkg_dir")"
        ;;
    *)
        exit_with "$target_hart has unknown TARGET=$pkg_target" 3
        ;;
    esac
    popd >/dev/null
    pushd "$(dirname "$pkg_artifact")" >/dev/null
    sha256sum "$(basename "$pkg_artifact")" > "${pkg_artifact}.sha256sum"
    popd
    declare -g archive_name
    declare -g pkg_artifact
}

get_hab_ident() {
    local target=$1
    buildkite-agent meta-data get "hab-ident-${target}"
}

has_hab_ident() {
    local target=$1
    buildkite-agent meta-data exists "hab-ident-${target}"
}

set_hab_ident() {
    local target=$1
    local ident=$2
    buildkite-agent meta-data set "hab-ident-${target}" "${ident}"
}

get_hab_artifact() {
    local target=$1
    buildkite-agent meta-data get "hab-artifact-${target}"
}

set_hab_artifact() {
    local target=$1
    local artifact=$2
    buildkite-agent meta-data set "hab-artifact-${target}" "${artifact}"
}

get_hab_release() {
    local target=$1
    buildkite-agent meta-data get "hab-release-${target}"
}

set_hab_release() {
    local target=$1
    local release=$2
    buildkite-agent meta-data set "hab-release-${target}" "${release}"
}

get_studio_ident() {
    local target=$1
    buildkite-agent meta-data get "studio-ident-${target}"
}

has_studio_ident() {
    local target=$1
    buildkite-agent meta-data exists "studio-ident-${target}"
}

set_studio_ident() {
    local target=$1
    local ident=$2
    buildkite-agent meta-data set "studio-ident-${target}" "${ident}"
}

get_backline_ident() {
    local target=$1
    buildkite-agent meta-data get "backline-ident-${target}"
}

set_backline_ident() {
    local target=$1
    local ident=$2
    buildkite-agent meta-data set "backline-ident-${target}" "${ident}"
}

get_backline_artifact() {
    local target=$1
    buildkite-agent meta-data get "backline-artifact-${target}"
}

set_backline_artifact() {
    local target=$1
    local ident=$2
    buildkite-agent meta-data set "backline-artifact-${target}" "${ident}"
}

get_release_channel() {
    echo "habitat-release-${BUILDKITE_BUILD_ID}"
}

get_version() {
    buildkite-agent meta-data get "version"
}

set_version() {
    local version=$1
    buildkite-agent meta-data set "version" "${version}"
}

# Until we can reliably deal with packages that have the same
# identifier, but different target, we'll track the information in
# Buildkite metadata.
#
# Each time we put a package into our release channel, we'll record
# what target it was built for.
set_target_metadata() {
    local package_ident="${1}"
    local target="${2}"

    echo "--- :partyparrot: Setting target metadata for '${package_ident}' (${target})"
    buildkite-agent meta-data set "${package_ident}-${target}" "true"
}

# When we do the final promotions, we need to know the target of each
# package in order to properly get the promotion done. If Buildkite metadata for
# an ident/target pair exists, then that means that's a valid
# combination, and we can use the target in the promotion call.
ident_has_target() {
    local package_ident="${1}"
    local target="${2}"

    echo "--- :partyparrot: Checking target metadata for '${package_ident}' (${target})"
    buildkite-agent meta-data exists "${package_ident}-${target}"
}