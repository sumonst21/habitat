#!/bin/bash

set -euo pipefail

# Test the Habitat tar exporter.
#
# Pass as arguments the name of the package to export, and the channel
# from which to pull the base packages (supervisor, launcher, etc.)
#
# Example:
#
#     test_tar_export.sh core/gzip stable
#

# Extract the identifier of a particular Habitat package in a tarball.
#
# Provide just the package name; the origin is always assumed to be
# "core"
#
#     echo $(extract_ident my.tar.gz hab)
#     # -> core/hab/0.83.0/20190712231625
extract_ident() {
    local tar_archive=${1}
    local package_name=${2}

    # Extract the path to the IDENT file for a given "core/XXX" package that
    # should be present in the tarball.
    local ident_file
    ident_file=$(tar \
                     --list \
                     --file=${tar_archive} | \
                 grep --basic-regexp \
                      "hab/pkgs/core/${package_name}/.*/.*/IDENT")

    if [[ -n "${ident_file}" ]]; then
        # Extract the *contents* of that IDENT file
        tar --extract \
            --to-stdout \
            --file=${tar_archive} \
            "${ident_file}"
    else
        return 1
    fi
}

pkg_ident="${1}"
channel="${2}"

# Remove tarball if already present
rm -f ./*.tar.gz

hab pkg export tar "${pkg_ident}" --base-pkgs-channel="${channel}"

tarball=$(find . -maxdepth 1 -type f -name "*.tar.gz")

# Check if tarball is present
if [ -f "${tarball}" ] ; then
    echo "--- Package was successfully exported to a tarball"
else
    echo "--- Package was NOT successfully exported"
    exit 1
fi

# Query contents for core Habitat packages
# TODO: validate these against expected values
hab_ident=$(extract_ident "${tarball}" hab)
echo "core/hab in tarball = ${hab_ident}"
launcher_ident=$(extract_ident "${tarball}" hab-launcher)
echo "core/hab-launcher in tarball = ${launcher_ident}"
sup_ident=$(extract_ident "${tarball}" hab-sup)
echo "core/hab-sup in tarball = ${sup_ident}"
