#!/usr/bin/env bash
# Checks what the built APK's manifest says, with the SDK's own aapt2: the one
# permission is INTERNET and nothing else (no camera, notifications, foreground
# service, network state or Bluetooth), backup and device transfer off, nothing exported but the launcher activity, and no service,
# receiver or provider. The manifest in the APK is the merged one: no library
# adds to it.
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

# 1. The one permission is INTERNET, and no feature that needs another.
declared="$(printf '%s\n' "$tree" | grep -E 'E: uses-permission' | wc -l)"
if [ "$declared" -eq 1 ]; then pass "exactly one uses-permission element"; else fail "$declared uses-permission elements"; fi
if printf '%s\n' "$tree" | grep -q 'android.permission.INTERNET'; then pass "the permission is INTERNET"; else fail "INTERNET is not declared"; fi
if printf '%s\n' "$tree" | grep -q -i -E 'NETWORK_STATE|WIFI_STATE|CHANGE_NETWORK|NEARBY|BLUETOOTH|NFC|CAMERA|NOTIFICATION|FOREGROUND|WAKE_LOCK|BOOT_COMPLETED|STORAGE'; then
    fail "a name other than INTERNET is in the manifest"
else
    pass "no other permission name in the manifest"
fi
perms="$("$aapt2" dump permissions "$apk" 2>&1 | grep -v -E '^package:' | sed -e 's/^ *//' | sort | tr '\n' ' ')"
if [ "$perms" = "uses-permission: name='android.permission.INTERNET' " ]; then pass "aapt2 dump permissions lists exactly INTERNET"; else fail "aapt2 dump permissions lists: $perms"; fi

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

# 3. Components: one activity, exported, the launcher; nothing else.
if [ "$(count 'E: activity( |$)')" -eq 1 ]; then pass "one activity"; else fail "not exactly one activity"; fi
for kind in service receiver provider activity-alias; do
    if [ "$(count "E: $kind( |\$)")" -eq 0 ]; then pass "no $kind"; else fail "a $kind is declared"; fi
done
exported="$(printf '%s\n' "$tree" | grep -c -E 'exported.*=true')"
unexported="$(printf '%s\n' "$tree" | grep -c -E 'exported.*=false')"
if [ "$exported" -eq 1 ] && [ "$unexported" -eq 0 ]; then pass "exactly one exported component"; else fail "exported components: $exported true, $unexported false"; fi
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
