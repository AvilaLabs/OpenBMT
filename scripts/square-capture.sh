#!/usr/bin/env bash
# Reads a Square seller account's locations, recent orders and recent
# payments, read-only, and saves the replies so that the order reader
# (crates/openbmt-pos-readers) can be checked against what Square really
# sends. It writes nothing to Square.
#
#   scripts/square-capture.sh <token file> [--sandbox] [--days N] [--out DIR]
#
# The token file holds one line: an access token for the seller's own
# application (Developer Console, Credentials), with ORDERS_READ,
# PAYMENTS_READ and MERCHANT_PROFILE_READ. It must be readable by its owner
# only (chmod 600). The token is given to curl on its standard input as a
# config file, so it is never on a command line, in the process list or in
# the saved files. The replies are saved under the output folder (default
# ~/.cache/openbmt/tmp/square-capture/<UTC time>), outside the repository:
# they hold the shop's orders, which are not for publishing.
#
# Square-Version is not sent, so the replies are in the version set for the
# application in the Developer Console; the script records that it did not
# set one.
set -euo pipefail

usage() {
    echo "usage: $0 <token file> [--sandbox] [--days N] [--out DIR]" >&2
    exit 2
}

[ $# -ge 1 ] || usage
token_file=$1
shift
base=https://connect.squareup.com
days=7
out=""
while [ $# -gt 0 ]; do
    case "$1" in
        --sandbox) base=https://connect.squareupsandbox.com; shift ;;
        --days) [ $# -ge 2 ] || usage; days=$2; shift 2 ;;
        --out) [ $# -ge 2 ] || usage; out=$2; shift 2 ;;
        *) usage ;;
    esac
done
case "$days" in ''|*[!0-9]*) echo "FAIL  --days must be a whole number" >&2; exit 2 ;; esac
if [ "$days" -lt 1 ] || [ "$days" -gt 31 ]; then
    echo "FAIL  --days must be 1 to 31" >&2
    exit 2
fi

if [ ! -f "$token_file" ]; then
    echo "FAIL  no token file at $token_file" >&2
    exit 1
fi
mode=$(stat -c %a "$token_file")
if [ "$mode" != 600 ] && [ "$mode" != 400 ]; then
    echo "FAIL  $token_file has mode $mode; run: chmod 600 $token_file" >&2
    exit 1
fi
token=$(head -n 1 "$token_file" | tr -d '\r\n')
case "$token" in
    ''|*[!A-Za-z0-9._-]*)
        echo "FAIL  the first line of $token_file is not an access token" >&2
        exit 1 ;;
esac

[ -n "$out" ] || out="$HOME/.cache/openbmt/tmp/square-capture/$(date -u +%Y%m%dT%H%M%SZ)"
mkdir -p "$out"
chmod 700 "$out"

# One request: the token and the headers go to curl on stdin as a config.
# $1 method, $2 path, $3 output file, $4 body file (optional).
call() {
    local method=$1 path=$2 file=$3 body=${4:-}
    local status
    {
        printf 'header = "Authorization: Bearer %s"\n' "$token"
        printf 'header = "Accept: application/json"\n'
        if [ -n "$body" ]; then
            printf 'header = "Content-Type: application/json"\n'
            printf 'data-binary = "@%s"\n' "$body"
        fi
    } | curl --config - --silent --show-error --proto '=https' --tlsv1.2 \
        --max-time 30 --max-filesize 8388608 --request "$method" \
        --output "$out/$file" --write-out '%{http_code}' "$base$path" > "$out/$file.status"
    status=$(cat "$out/$file.status")
    if [ "$status" != 200 ]; then
        echo "FAIL  $method $path answered $status (reply in $out/$file)" >&2
        exit 1
    fi
    echo "PASS  $method $path ($(stat -c %s "$out/$file") bytes)"
}

since=$(date -u -d "-$days days" +%Y-%m-%dT%H:%M:%SZ)

call GET /v2/locations locations.json
locations=$(python3 -I -c '
import json, sys
d = json.load(open(sys.argv[1]))
print(json.dumps([l["id"] for l in d.get("locations", [])][:10]))
' "$out/locations.json")
if [ "$locations" = "[]" ]; then
    echo "FAIL  the account has no locations" >&2
    exit 1
fi

python3 -I -c '
import json, sys
body = {
    "location_ids": json.loads(sys.argv[1]),
    "query": {
        "filter": {
            "state_filter": {"states": ["COMPLETED", "CANCELED"]},
            "date_time_filter": {"updated_at": {"start_at": sys.argv[2]}},
        },
        "sort": {"sort_field": "UPDATED_AT", "sort_order": "ASC"},
    },
    "limit": 100,
}
json.dump(body, open(sys.argv[3], "w"))
' "$locations" "$since" "$out/search-request.json"
call POST /v2/orders/search orders.json "$out/search-request.json"

first=$(python3 -I -c 'import json,sys; print(json.loads(sys.argv[1])[0])' "$locations")
call GET "/v2/payments?begin_time=$since&location_id=$first&limit=100" payments.json

rm -f "$out"/*.status
{
    echo "base: $base"
    echo "since: $since"
    echo "square-version: not sent (the application's own version)"
    echo "captured: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
} > "$out/capture.txt"
echo "PASS  replies saved in $out (not for the repository)"
