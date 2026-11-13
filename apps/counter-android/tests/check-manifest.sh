#!/usr/bin/env bash
# Checks what the built APK's manifest says, with the SDK's own aapt2: the
# permissions are INTERNET, CAMERA and NFC and nothing else (no notifications,
# foreground service, network state or Bluetooth), the camera and card emulation are not required, backup and device transfer off, nothing exported but the
# launcher activity and the NFC tag service (TP-10), no receiver, one service (the tag, bound only by
# BIND_NFC_SERVICE, one application ID, category other, device unlock required, no NFC discovery filter, TP-23)
# and one provider (the image of a request's code, CR-214), not exported. The manifest in the APK is the merged one: no library
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

# 1. The permissions are INTERNET, CAMERA and NFC, and the camera is not required.
declared="$(printf '%s\n' "$tree" | grep -E 'E: uses-permission' | wc -l)"
if [ "$declared" -eq 3 ]; then pass "exactly three uses-permission elements"; else fail "$declared uses-permission elements"; fi
for p in INTERNET CAMERA NFC; do
    if printf '%s\n' "$tree" | grep -q "android.permission.$p"; then pass "the permission $p is declared"; else fail "$p is not declared"; fi
done
if printf '%s\n' "$tree" | grep -q -i -E 'NETWORK_STATE|WIFI_STATE|CHANGE_NETWORK|NEARBY|BLUETOOTH|NOTIFICATION|FOREGROUND|WAKE_LOCK|BOOT_COMPLETED|STORAGE|RECORD_AUDIO|LOCATION'; then
    fail "a name other than INTERNET, CAMERA and NFC is in the manifest"
else
    pass "no other permission name in the manifest"
fi
perms="$("$aapt2" dump permissions "$apk" 2>&1 | grep -v -E '^package:' | sed -e 's/^ *//' | sort | tr '\n' ' ')"
if [ "$perms" = "uses-permission: name='android.permission.CAMERA' uses-permission: name='android.permission.INTERNET' uses-permission: name='android.permission.NFC' " ]; then
    pass "aapt2 dump permissions lists exactly CAMERA, INTERNET and NFC"
else
    fail "aapt2 dump permissions lists: $perms"
fi
# Three uses-feature elements, all not required: camera.any, and camera,
# which the CAMERA permission would otherwise imply as required (a store
# would then hide the app from a phone without a back camera), and nfc.hce, so
# that a phone without card emulation still installs the app.
if [ "$(count 'E: uses-feature')" -eq 3 ] \
    && printf '%s\n' "$tree" | grep -q '"android.hardware.camera.any"' \
    && printf '%s\n' "$tree" | grep -q '"android.hardware.camera"' \
    && printf '%s\n' "$tree" | grep -q '"android.hardware.nfc.hce"'; then
    pass "exactly three uses-feature elements, android.hardware.camera.any, android.hardware.camera and android.hardware.nfc.hce"
else
    fail "the uses-feature elements are not exactly android.hardware.camera.any, android.hardware.camera and android.hardware.nfc.hce"
fi
badging="$("$aapt2" dump badging "$apk" 2>&1)"
if printf '%s\n' "$badging" | grep -q "uses-feature-not-required: name='android.hardware.camera.any'" \
    && printf '%s\n' "$badging" | grep -q "uses-feature-not-required: name='android.hardware.camera'" \
    && printf '%s\n' "$badging" | grep -q "uses-feature-not-required: name='android.hardware.nfc.hce'" \
    && ! printf '%s\n' "$badging" | grep -q -E "^uses-feature: name='android.hardware.(camera|nfc)"; then
    pass "badging lists the camera and card emulation as not required, with no required feature of either"
else
    fail "badging lists a required camera or card emulation feature"
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

# 3. Components: one activity, exported, the launcher; one service, the NFC tag; nothing else.
if [ "$(count 'E: activity( |$)')" -eq 1 ]; then pass "one activity"; else fail "not exactly one activity"; fi
for kind in receiver activity-alias; do
    if [ "$(count "E: $kind( |\$)")" -eq 0 ]; then pass "no $kind"; else fail "a $kind is declared"; fi
done
if [ "$(count 'E: provider( |$)')" -eq 1 ] \
    && printf '%s\n' "$tree" | grep -q 'org.openbmt.counter.app.ShareImageProvider' \
    && printf '%s\n' "$tree" | grep -q -E 'grantUriPermissions.*=true'; then
    pass "one provider, the share image's"
else
    fail "the provider is not exactly the share image's"
fi
if [ "$(count 'E: service( |$)')" -eq 1 ] \
    && printf '%s\n' "$tree" | grep -q 'org.openbmt.counter.app.TapService' \
    && printf '%s\n' "$tree" | grep -q 'android.permission.BIND_NFC_SERVICE' \
    && printf '%s\n' "$tree" | grep -q 'android.nfc.cardemulation.action.HOST_APDU_SERVICE'; then
    pass "one service, the NFC tag's, bound by BIND_NFC_SERVICE"
else
    fail "the service is not exactly the NFC tag's"
fi
if printf '%s\n' "$tree" | grep -q -E 'NDEF_DISCOVERED|TECH_DISCOVERED|TAG_DISCOVERED|nfc\.action'; then
    fail "a filter for tags, tag technologies or NDEF messages is declared"
else
    pass "no NFC discovery filter (TP-23)"
fi
if unzip -l "$apk" | grep -q 'res/xml/tap_aid.xml'; then
    aid="$("$aapt2" dump xmltree --file res/xml/tap_aid.xml "$apk" 2>&1)"
    if [ "$(printf '%s\n' "$aid" | grep -c 'E: aid-filter')" -eq 1 ] \
        && printf '%s\n' "$aid" | grep -q 'D2760000850101' \
        && [ "$(printf '%s\n' "$aid" | grep -c 'E: aid-group')" -eq 1 ] \
        && printf '%s\n' "$aid" | grep -q '"other"' \
        && printf '%s\n' "$aid" | grep -q -E 'requireDeviceUnlock.*=true'; then
        pass "one application ID, category other, device unlock required"
    else
        fail "the tag's service file is not one application ID of category other with device unlock required"
    fi
else
    fail "res/xml/tap_aid.xml is not in the APK"
fi
exported="$(printf '%s\n' "$tree" | grep -c -E 'exported.*=true')"
unexported="$(printf '%s\n' "$tree" | grep -c -E 'exported.*=false')"
if [ "$exported" -eq 2 ] && [ "$unexported" -eq 1 ]; then pass "exactly two exported components (the launcher and the tag service), and the provider not exported"; else fail "exported components: $exported true, $unexported false"; fi
if printf '%s\n' "$tree" | grep -q 'android.intent.category.LAUNCHER'; then pass "the exported one is the launcher"; else fail "no launcher category"; fi
if [ "$(count 'E: intent-filter')" -eq 2 ] && [ "$(count 'E: action')" -eq 2 ]; then pass "two intent filters with one action each (the launcher's and the tag service's)"; else fail "more intent filters or actions than the launcher's and the tag service's"; fi

# 4. The SDK levels.
badging="$("$aapt2" dump badging "$apk" 2>&1)"
for want in "minSdkVersion:'29'" "targetSdkVersion:'36'"; do
    if printf '%s\n' "$badging" | grep -q "$want"; then pass "$want"; else fail "$want missing from badging"; fi
done

echo
if [ "$fails" -eq 0 ]; then echo "manifest checks: all passed"; else echo "manifest checks: $fails failed"; fi
exit $((fails > 0))
