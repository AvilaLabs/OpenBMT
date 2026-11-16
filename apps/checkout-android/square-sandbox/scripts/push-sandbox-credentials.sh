#!/usr/bin/env bash
set -euo pipefail

credentials_file=${SQUARE_SANDBOX_CREDENTIALS:-}
if [[ -z "$credentials_file" || ! -f "$credentials_file" ]]; then
  echo "Set SQUARE_SANDBOX_CREDENTIALS to an external Sandbox properties file." >&2
  exit 2
fi

if ! awk -F= '
  /^[[:space:]]*#/ || /^[[:space:]]*$/ { next }
  {
    key=$1; value=substr($0, index($0, "=")+1)
    gsub(/^[[:space:]]+|[[:space:]]+$/, "", key)
    gsub(/^[[:space:]]+|[[:space:]]+$/, "", value)
    if (key == "applicationId") app=value
    if (key == "accessToken") token=value
    if (key == "locationId") location=value
  }
  END { exit !(app ~ /^sandbox-sq0idb-.+/ && token != "" && location != "") }
' "$credentials_file"; then
  echo "Credential file must contain a Sandbox applicationId, accessToken, and locationId." >&2
  exit 2
fi

adb_bin=${ADB:-adb}
package_name=org.openbmt.checkout.square.sandbox.debug
if [[ ${OPENBMT_SANDBOX_TRIAL+x} ]]; then
  if [[ ! $OPENBMT_SANDBOX_TRIAL =~ ^[a-z][a-z0-9_]{0,15}$ ]]; then
    echo "OPENBMT_SANDBOX_TRIAL must match [a-z][a-z0-9_]{0,15}." >&2
    exit 2
  fi
  package_name+=".$OPENBMT_SANDBOX_TRIAL"
fi
if ! "$adb_bin" shell run-as "$package_name" true >/dev/null 2>&1; then
  echo "Install and launch the debug Sandbox checkout APK on a device first." >&2
  exit 1
fi

# Stream directly to the app-private noBackupFilesDir; never put the token in
# the APK, repository, host command arguments, or a shared Android temp file.
cat -- "$credentials_file" | "$adb_bin" exec-in run-as "$package_name" sh -c \
  'mkdir -p no_backup && cat > no_backup/square-sandbox.properties && chmod 600 no_backup/square-sandbox.properties'
echo "Sandbox credentials imported into the debug app's private storage."
