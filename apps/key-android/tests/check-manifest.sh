#!/usr/bin/env bash
# Checks what the built APK's manifest says, with the SDK's own aapt2: no
# permission but CAMERA (with the camera feature not required), no network permission or setting, backup and device
# transfer off, nothing exported but the launcher activity, and no service,
# receiver or provider. The manifest in the APK is the merged one: no library
# adds to it.
#
# A release APK (the default) must also be the plain Key app: the package
# org.openbmt.key and not one word of the development copy's band (SR-91).
# With the third argument "dev" the APK must be the development copy instead.
#
# Usage: tests/check-manifest.sh APK [AAPT2 [release|dev]]

set -u

apk="${1:?usage: check-manifest.sh APK [AAPT2]}"
aapt2="${2:-${AAPT2:-aapt2}}"
variant="${3:-release}"
band="Development copy. Not for real money."
fails=0

fail() { printf 'FAIL  %s\n' "$1"; fails=$((fails + 1)); }
pass() { printf 'PASS  %s\n' "$1"; }

tree="$("$aapt2" dump xmltree --file AndroidManifest.xml "$apk" 2>&1)" || { echo "FAIL  aapt2 could not read the manifest"; exit 1; }

count() { printf '%s\n' "$tree" | grep -c -E "$1"; }

# 1. The one permission is CAMERA, and the camera is not required. No network permission.
declared="$(printf '%s\n' "$tree" | grep -E 'E: uses-permission' | wc -l)"
if [ "$declared" -eq 1 ]; then pass "exactly one uses-permission element"; else fail "$declared uses-permission elements"; fi
if printf '%s\n' "$tree" | grep -q 'android.permission.CAMERA'; then pass "the permission is CAMERA"; else fail "CAMERA is not declared"; fi
if printf '%s\n' "$tree" | grep -q -i -E 'INTERNET|NETWORK_STATE|WIFI_STATE|CHANGE_NETWORK|NEARBY|BLUETOOTH|NFC'; then
    fail "a network-related name is in the manifest"
else
    pass "no network-related name in the manifest"
fi
perms="$("$aapt2" dump permissions "$apk" 2>&1 | grep -v -E '^package:' | sed -e 's/^ *//' | sort | tr '\n' ' ')"
if [ "$perms" = "uses-permission: name='android.permission.CAMERA' " ]; then pass "aapt2 dump permissions lists exactly CAMERA"; else fail "aapt2 dump permissions lists: $perms"; fi
# Two uses-feature elements, both not required: camera.any, and camera,
# which the CAMERA permission would otherwise imply as required (a store
# would then hide the app from a phone without a back camera).
if [ "$(count 'E: uses-feature')" -eq 2 ] \
    && printf '%s\n' "$tree" | grep -q '"android.hardware.camera.any"' \
    && printf '%s\n' "$tree" | grep -q '"android.hardware.camera"'; then
    pass "exactly two uses-feature elements, android.hardware.camera.any and android.hardware.camera"
else
    fail "the uses-feature elements are not exactly android.hardware.camera.any and android.hardware.camera"
fi
badging="$("$aapt2" dump badging "$apk" 2>&1)"
if printf '%s\n' "$badging" | grep -q "uses-feature-not-required: name='android.hardware.camera.any'" \
    && printf '%s\n' "$badging" | grep -q "uses-feature-not-required: name='android.hardware.camera'" \
    && ! printf '%s\n' "$badging" | grep -q -E "^uses-feature: name='android.hardware.camera"; then
    pass "badging lists the camera as not required, with no required camera feature"
else
    fail "badging lists a required camera feature"
fi

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

# 5. The development copy is never a release (SR-91).
pkg="$(printf '%s\n' "$badging" | sed -n "s/^package: name='\([^']*\)'.*/\1/p")"
in_dex=0
for d in $(unzip -Z1 "$apk" | grep -E '^classes[0-9]*\.dex$'); do
    if unzip -p "$apk" "$d" | grep -a -q -F "$band"; then in_dex=1; fi
done
if [ "$variant" = dev ]; then
    if [ "$pkg" = "org.openbmt.key.dev" ]; then pass "the package is org.openbmt.key.dev"; else fail "the development copy's package is $pkg"; fi
    if printf '%s\n' "$badging" | grep -q "application-label:'Key (dev)'"; then pass "the label is Key (dev)"; else fail "the development copy's label is not Key (dev)"; fi
    if [ "$in_dex" -eq 1 ]; then pass "the band's words are in the code"; else fail "the band's words are not in the code"; fi
else
    if [ "$pkg" = "org.openbmt.key" ]; then pass "the package is org.openbmt.key"; else fail "a release APK has the package $pkg"; fi
    if [ "$in_dex" -eq 0 ]; then pass "the development copy's band is not in the code"; else fail "a release APK has the development copy's band"; fi
    if printf '%s\n' "$badging" | grep -q "application-label:'Key (dev)'"; then fail "a release APK is labelled Key (dev)"; else pass "the label is not Key (dev)"; fi
fi

echo
if [ "$fails" -eq 0 ]; then echo "manifest checks: all passed"; else echo "manifest checks: $fails failed"; fi
exit $((fails > 0))
