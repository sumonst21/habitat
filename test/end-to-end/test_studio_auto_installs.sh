#!/bin/bash

set -euo pipefail

export HAB_BLDR_URL="${ACCEPTANCE_HAB_BLDR_URL}"
export HAB_ORIGIN="ci"

# Ensure there are no studios installed
while [ -d /hab/pkgs/core/hab-studio ]; do 
  hab pkg uninstall core/hab-studio
done

echo "--- Generating signing key"
# Without a signing key, the studio will be created but
# will exit 1 as it is unable to import a signing key for 
# $HAB_ORIGIN
hab origin key generate $HAB_ORIGIN

echo "--- Creating new studio"

hab studio new

echo "--- $(hab studio version)"
