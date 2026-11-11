#!/usr/bin/env bash
# Checks what the built APK's manifest says, with the SDK's own aapt2: the
# permissions are INTERNET, CAMERA, FOREGROUND_SERVICE,
# FOREGROUND_SERVICE_SPECIAL_USE and POST_NOTIFICATIONS and nothing else (no
# network state, wake lock, boot, storage or Bluetooth), the camera is a
# feature that is not required, backup and device transfer off, cleartext
# traffic off, not debuggable, nothing exported but the launcher activity, one
# service of type specialUse that is not exported and names its use, and no
# receiver, provider or activity alias. The manifest in the APK is the merged
# one: no library adds to it. (OA-02, OA-03, OA-89)
#
# Usage: tests/check-manifest.sh APK [AAPT2]

set -u

apk="${1:?usage: check-manifest.sh APK [AAPT2]}"
aapt2="${2:-${AAPT2:-aapt2}}"
fails=0

fail() { printf 'FAIL  %s\n' "$1"; fails=$((fails + 1)); }
pass() { printf 'PASS  %s\n' "$1"; }

tree="$("$aapt2" dump xmltree --file AndroidManifest.xml "$apk" 2>&1)" || { echo "FAIL  aapt2 could not read the manifest"; exit 1; }

count() { printf '%s\n' "$tree" | grep -c -E "$1"; }

# 1. The five permissions, and the camera is not required.
declared="$(printf '%s\n' "$tree" | grep -E 'E: uses-permission' | wc -l)"
if [ "$declared" -eq 5 ]; then pass "exactly five uses-permission elements"; else fail "$declared uses-permission elements"; fi
for p in INTERNET CAMERA FOREGROUND_SERVICE FOREGROUND_SERVICE_SPECIAL_USE POST_NOTIFICATIONS; do
    if printf '%s\n' "$tree" | grep -q "android.permission.$p"; then pass "$p is declared"; else fail "$p is not declared"; fi
done
if printf '%s\n' "$tree" | grep -q -i -E 'NETWORK_STATE|WIFI_STATE|CHANGE_NETWORK|NEARBY|BLUETOOTH|NFC|WAKE_LOCK|BOOT_COMPLETED|STORAGE|RECORD_AUDIO|LOCATION|SCHEDULE_EXACT|ALARM|REQUEST_IGNORE_BATTERY'; then
    fail "a name other than the five permissions is in the manifest"
else
    pass "no other permission name in the manifest"
fi
perms="$("$aapt2" dump permissions "$apk" 2>&1 | grep -v -E '^package:' | sed -e 's/^ *//' | sort | tr '\n' ' ')"
want="uses-permission: name='android.permission.CAMERA' uses-permission: name='android.permission.FOREGROUND_SERVICE' uses-permission: name='android.permission.FOREGROUND_SERVICE_SPECIAL_USE' uses-permission: name='android.permission.INTERNET' uses-permission: name='android.permission.POST_NOTIFICATIONS' "
if [ "$perms" = "$want" ]; then
    pass "aapt2 dump permissions lists exactly the five"
else
    fail "aapt2 dump permissions lists: $perms"
fi
if [ "$(count 'E: uses-feature')" -eq 1 ] && printf '%s\n' "$tree" | grep -q 'android.hardware.camera.any'; then
    pass "one uses-feature: camera.any"
else
    fail "the features are not exactly camera.any"
fi
if printf '%s\n' "$tree" | grep -E 'E: uses-feature' -A3 | grep -q -E 'required.*=false'; then pass "the camera is not required"; else fail "the camera is required"; fi

# 2. Backup and data extraction.
if printf '%s\n' "$tree" | grep -q -E 'allowBackup.*=false'; then pass "allowBackup is false"; else fail "allowBackup is not false"; fi
if printf '%s\n' "$tree" | grep -q -E 'dataExtractionRules'; then pass "dataExtractionRules is set"; else fail "dataExtractionRules is missing"; fi
if printf '%s\n' "$tree" | grep -q -E 'usesCleartextTraffic.*=false'; then pass "cleartext traffic is off"; else fail "cleartext traffic is not off"; fi
if printf '%s\n' "$tree" | grep -q -E 'debuggable.*=true'; then fail "the app is debuggable"; else pass "the app is not debuggable"; fi

if unzip -l "$apk" | grep -q 'res/xml/data_extraction_rules.xml'; then
    dump="$("$aapt2" dump xmltree --file res/xml/data_extraction_rules.xml "$apk" 2>&1)"
    if printf '%s\n' "$dump" | grep -q 'E: include'; then fail "data extraction rules include something"; else pass "data extraction rules include nothing"; fi
    for place in cloud-backup device-transfer; do
        if printf '%s\n' "$dump" | grep -q "E: $place"; then pass "rules cover $place"; else fail "rules do not cover $place"; fi
    done
    excluded="$(printf '%s\n' "$dump" | grep -c 'E: exclude')"
    if [ "$excluded" -ge 10 ]; then pass "every domain is excluded ($excluded excludes)"; else fail "too few excludes ($excluded)"; fi
else
    fail "res/xml/data_extraction_rules.xml is not in the APK"
fi

# 3. Components: one activity, exported, the launcher; one service, not exported; nothing else.
if [ "$(count 'E: activity( |$)')" -eq 1 ]; then pass "one activity"; else fail "not exactly one activity"; fi
if [ "$(count 'E: service( |$)')" -eq 1 ]; then pass "one service"; else fail "not exactly one service"; fi
if printf '%s\n' "$tree" | grep -q -E 'foregroundServiceType.*0x40000000'; then pass "the service is of type specialUse"; else fail "the service is not of type specialUse only"; fi
if printf '%s\n' "$tree" | grep -q "PROPERTY_SPECIAL_USE_FGS_SUBTYPE" && printf '%s\n' "$tree" | grep -q "Follows the shop's own wallet and tells the shop's counters it is watching"; then pass "the service names its use"; else fail "the service does not name its use"; fi
if [ "$(count 'E: property( |$)')" -eq 1 ]; then pass "one property"; else fail "not exactly one property"; fi
for kind in receiver provider activity-alias; do
    if [ "$(count "E: $kind( |\$)")" -eq 0 ]; then pass "no $kind"; else fail "a $kind is declared"; fi
done
exported="$(printf '%s\n' "$tree" | grep -c -E 'exported.*=true')"
unexported="$(printf '%s\n' "$tree" | grep -c -E 'exported.*=false')"
if [ "$exported" -eq 1 ] && [ "$unexported" -eq 1 ]; then pass "one exported component, and the service not exported"; else fail "exported components: $exported true, $unexported false"; fi
if printf '%s\n' "$tree" | grep -q 'android.intent.category.LAUNCHER'; then pass "the exported one is the launcher"; else fail "no launcher category"; fi
if [ "$(count 'E: intent-filter')" -eq 1 ] && [ "$(count 'E: action')" -eq 1 ]; then pass "one intent filter with one action"; else fail "more intent filters or actions than the launcher's"; fi

# 4. The SDK levels.
badging="$("$aapt2" dump badging "$apk" 2>&1)"
for want in "minSdkVersion:'29'" "targetSdkVersion:'36'"; do
    if printf '%s\n' "$badging" | grep -q "$want"; then pass "$want"; else fail "$want missing from badging"; fi
done

echo
if [ "$fails" -eq 0 ]; then echo "manifest checks: all passed"; else echo "manifest checks: $fails failed"; fi
exit $((fails > 0))
