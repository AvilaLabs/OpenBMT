#!/usr/bin/env bash
# The Box, paired and used over TCP: the invitation, the claim, the welcome,
# the device list and the Box's relay (docs/BOX-RULES.md sections 1 to 5).
#
# Question: does a Box that starts with nothing offer nothing to a stranger,
# pair from an invitation typed into it and a welcome made by the Key app,
# fingerprint words that agree on both sides, pass sealed messages between
# the devices on its list and refuse the others, follow new settings (a device
# taken off the list loses its mailbox), keep its key and its members across a
# restart, refuse a second process on its directory, turn itself off when the
# settings drop it, and refuse a mistyped or a different invitation?
#
# No Bitcoin Core is needed. The Box is the `openbmt-box` binary. The Key app
# and the owner phone are the `box_pair` example of `openbmt-box`, which uses
# the core functions and the link's client; it makes the invitation, checks
# the claim, builds the first settings (the Box listed as a Box, signed by both
# roots) and sends the welcome. The script carries nothing between the two:
# the invitation is typed into the Box's console (its standard input), and
# everything else goes through the Box's TCP port.
#
# The steps (printed as a table at the end):
#   1. A stranger can say hello to an unpaired Box and gets "no claim"; it
#      cannot make a mailbox or put a message.
#   2. An invitation is entered, the claim is fetched and verified, the
#      welcome is sent, the Box says paired, and the four fingerprint words
#      the Box printed are the ones the Key app side shows.
#   3. Two devices on the settings' list make mailboxes and pass sealed
#      messages through the Box; a device not on the list cannot.
#   4. New settings, sent as a sealed Settings message from the owner, remove
#      one device: its mailbox is gone and it is refused.
#   5. A restart on the same directory: still paired, the same key, the same
#      members.
#   6. A second `openbmt-box` on the same directory is refused.
#   7. Settings that drop the Box: its state is "turned off" and its members
#      are empty.
#   9. A stranger sends welcomes while an invitation is open: refused, the
#      same claim is still offered, and the real welcome then still pairs.
#   8. On a second Box: an invitation typed with one character changed is
#      refused by its check, and a different valid invitation makes the Key
#      app side reject the claim.
#  15. A Box run with --no-console (no standard input, an invitation only
#      from a file), ended by SIGKILL after the invitation, the welcome, the
#      mailboxes, a message and new settings: every restart finds the same
#      key and the right state with no error line, a message accepted before
#      a kill is read after it (BX-52), and SIGTERM ends it with exit 143.
#  17. A Box started with --status-page: GET / answers with the security
#      headers and words that say it is waiting for an invitation; another
#      path is 404 and another method 405; the setup code is printed after
#      the page's address and is on no page, a wrong code is refused with
#      words and holds the page for ten seconds; a wrong invitation posted
#      with the right code from the page's own origin is refused with words; a right one posted with a
#      foreign Origin, or with none, is refused and changes nothing; the
#      right one from the page's own origin is taken, the Key app side claims
#      and welcomes it, and the page then says paired, with no invitation on
#      it (BX-54 to BX-61).
#  18. The phones' libraries pair a real Box process: `openbmt-key-ffi` makes
#      the invitation, which is typed into the Box's console; `openbmt-owner-ffi`
#      fetches the claim; the Key app library checks it and welcomes the Box
#      in an approval session; the owner library hands the Box its welcome,
#      and the Box goes from unpaired to paired with the words the phones
#      showed; the owner's link then puts a sealed message through the Box
#      and fetches it (OA-40 to OA-47). It runs the ignored test
#      `oa_41_to_oa_43_a_real_box_process_is_paired` of `openbmt-owner-ffi`.
#
# Environment:
#   DRILL_DIR      scratch directory (default: a new one under ~/.cache/openbmt)
#   DRILL_PORTS    first of three free ports on 127.0.0.1 (default 18986)
#   DRILL_KEEP=1   keep the scratch directory afterwards
#   BOX_BIN        the `openbmt-box` binary
#   PAIR_BIN       the `box_pair` example binary
#   CARGO_TARGET_DIR  where a build goes (default ~/.cache/openbmt/target/box-e2e)
#
# Everything here is on 127.0.0.1 with keys made from fixed test names or
# from the system's randomness. No real keys or funds are involved. The
# processes and the scratch directory are removed on every exit.

set -uo pipefail

P0=${DRILL_PORTS:-18986}
PORT1=$P0
PORT2=$((P0 + 1))
PORT3=$((P0 + 2))
mkdir -p "$HOME/.cache/openbmt"
BASE=${DRILL_DIR:-$(mktemp -d "$HOME/.cache/openbmt/box-e2e-XXXXXX")}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
B1="$BASE/box1"
B2="$BASE/box2"
K1="$BASE/keyapp1"
K2="$BASE/keyapp2"
K3="$BASE/keyapp3"
K4="$BASE/keyapp4"
B3="$BASE/box3"
B4="$BASE/box4"

say() { printf '\n== %s\n' "$*"; }
note() { printf '      %s\n' "$*"; }
die() {
  printf 'ABORT %s\n' "$*" >&2
  exit 2
}

declare -A RESULT MESSAGE
pass() { RESULT[$1]=PASS; MESSAGE[$1]=$2; printf 'PASS  step %s: %s\n' "$1" "$2"; }
bad() { RESULT[$1]=FAIL; MESSAGE[$1]=$2; printf 'FAIL  step %s: %s\n' "$1" "$2"; }
notrun() { RESULT[$1]="NOT RUN"; MESSAGE[$1]=$2; printf 'NOT RUN  step %s: %s\n' "$1" "$2"; }

mkdir -p "$B1" "$B2" "$B3" "$B4" "$K1" "$K2" "$K3" "$K4"
declare -A PID LOG FDN
PID[1]=; PID[2]=
FDN[1]=3; FDN[2]=4
LAST=

stop_pid() { # pid
  local pid=$1
  [ -n "$pid" ] || return 0
  if kill -0 "$pid" 2>/dev/null; then
    kill "$pid" 2>/dev/null || true
    for _ in $(seq 1 20); do
      kill -0 "$pid" 2>/dev/null || break
      sleep 0.25
    done
    kill -9 "$pid" 2>/dev/null || true
  fi
}

cleanup() {
  exec 3>&- 4>&- 2>/dev/null || true
  stop_pid "${PID[1]:-}"
  stop_pid "${PID[2]:-}"
  stop_pid "${PG_PID:-}"
  if [ "${DRILL_KEEP:-0}" != 1 ]; then rm -rf -- "$BASE"; fi
}
trap cleanup EXIT

TARGET=${CARGO_TARGET_DIR:-$HOME/.cache/openbmt/target/box-e2e}
build() {
  local guard=$HOME/.cache/openbmt/guard.sh
  if [ -x "$guard" ]; then
    "$guard" env CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked \
      --manifest-path "$ROOT/Cargo.toml" -p openbmt-box --bins --example box_pair ||
      die "the build failed"
  else
    CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked \
      --manifest-path "$ROOT/Cargo.toml" -p openbmt-box --bins --example box_pair ||
      die "the build failed"
  fi
}
if [ -z "${BOX_BIN:-}" ] || [ -z "${PAIR_BIN:-}" ]; then
  say "Build the Box and the Key app example"
  build
  [ -n "${BOX_BIN:-}" ] || BOX_BIN="$TARGET/debug/openbmt-box"
  [ -n "${PAIR_BIN:-}" ] || PAIR_BIN="$TARGET/debug/examples/box_pair"
fi
for bin in BOX_BIN PAIR_BIN; do
  [ -x "${!bin}" ] || die "no binary at ${!bin} ($bin)"
done

# ---- helpers ------------------------------------------------------------------

# Start a Box. Its standard input is a fifo held open on descriptor 3 (Box 1)
# or 4 (Box 2), so that console commands can be sent.
start_box() { # n data-dir port
  local n=$1 fifo="$BASE/stdin-$1"
  LOG[$n]="$BASE/box-$n.log"
  : >"${LOG[$n]}"
  rm -f "$fifo"
  mkfifo "$fifo"
  if [ "$n" = 1 ]; then exec 3<>"$fifo"; else exec 4<>"$fifo"; fi
  if [ "$n" = 1 ]; then
    "$BOX_BIN" "$2" "127.0.0.1:$3" <&3 >"${LOG[$n]}" 2>&1 &
  else
    "$BOX_BIN" "$2" "127.0.0.1:$3" <&4 >"${LOG[$n]}" 2>&1 &
  fi
  PID[$n]=$!
  for _ in $(seq 1 40); do
    grep -q '^state: ' "${LOG[$n]}" && return 0
    kill -0 "${PID[$n]}" 2>/dev/null || return 1
    sleep 0.25
  done
  return 1
}
# Send a console command and wait for a new line that starts with `prefix`.
# LAST is the newest such line.
ask() { # n command prefix
  local n=$1 before i fd=${FDN[$1]}
  before=$(grep -c "^$3" "${LOG[$n]}")
  echo "$2" >&"$fd"
  for i in $(seq 1 40); do
    if [ "$(grep -c "^$3" "${LOG[$n]}")" -gt "$before" ]; then
      LAST=$(grep "^$3" "${LOG[$n]}" | tail -1)
      return 0
    fi
    sleep 0.25
  done
  LAST=
  return 1
}
state_of() { # n -> prints the state words
  ask "$1" state 'state: ' || return 1
  printf '%s' "${LAST#state: }"
}
# Wait for the Box's state to be `want`.
await_state() { # n want seconds
  local i s
  for i in $(seq 1 $(($3 * 4))); do
    s=$(state_of "$1")
    [ "$s" = "$2" ] && return 0
    sleep 0.25
  done
  return 1
}
# The member keys the Box reports, sorted, one per line.
members_of() { # n
  local before
  before=$(grep -c '^members: ' "${LOG[$1]}")
  ask "$1" members 'members: ' || return 1
  sleep 0.3
  local count=${LAST#members: }
  grep '^member: ' "${LOG[$1]}" | tail -n "$count" | sed 's/^member: //' | sort
}
member_count() { # n
  ask "$1" members 'members: ' || return 1
  printf '%s' "${LAST#members: }"
}
stop_box() { # n
  local n=$1
  echo quit >&"${FDN[$n]}"
  for _ in $(seq 1 40); do
    kill -0 "${PID[$n]}" 2>/dev/null || break
    sleep 0.25
  done
  wait "${PID[$n]}" 2>/dev/null
  local rc=$?
  PID[$n]=
  if [ "$n" = 1 ]; then exec 3>&-; else exec 4>&-; fi
  return $rc
}
pair() { "$PAIR_BIN" "$@" 2>&1; }
# Ask until the newest line starting with `prefix` has `pattern` in it.
await_line() { # n command prefix pattern seconds
  local i
  for i in $(seq 1 $(($5 * 2))); do
    ask "$1" "$2" "$3" && grep -q -- "$4" <<<"$LAST" && return 0
    sleep 0.5
  done
  return 1
}

# ---- 1 ---------------------------------------------------------------------------

say "1. An unpaired Box and a stranger"
start_box 1 "$B1" "$PORT1" || die "the Box did not start: $(cat "${LOG[1]}")"
FP1=$(sed -n 's/^fingerprint: //p' "${LOG[1]}")
KEY1=$(sed -n 's/^key: //p' "${LOG[1]}")
note "Box key $KEY1, fingerprint words: $FP1"
S1=$(state_of 1)
CLAIM_NONE=$(pair claim "$K1" "127.0.0.1:$PORT1")
MB_STRANGER=$(pair mailbox "$K1" "127.0.0.1:$PORT1" stranger)
SEND_STRANGER=$(pair send "$K1" "127.0.0.1:$PORT1" stranger owner hello)
note "claim: $CLAIM_NONE"
note "mailbox: $MB_STRANGER"
note "put: $SEND_STRANGER"
if [ "$S1" = unpaired ] && grep -q 'no open invitation' <<<"$CLAIM_NONE" &&
  grep -q '^refused' <<<"$MB_STRANGER" && grep -q '^refused' <<<"$SEND_STRANGER"; then
  pass 1 "an unpaired Box said hello (the Key app example checks its signature), answered a claim request with no claim, and refused a mailbox and a message"
else
  bad 1 "state '$S1'; claim: $CLAIM_NONE; mailbox: $MB_STRANGER; put: $SEND_STRANGER"
fi

# ---- 2 ---------------------------------------------------------------------------

say "2. Invitation, claim, welcome"
INV=$(pair invite "$K1")
note "invitation (typed into the Box's console): $INV"
ask 1 "invite $INV" 'invitation' || note "no answer to the invitation"
S2a=$(state_of 1)
CLAIM_OUT=$(pair claim "$K1" "127.0.0.1:$PORT1")
note "$(sed 's/^/key app: /' <<<"$CLAIM_OUT")"
KEYAPP_FP=$(sed -n 's/^box fingerprint: //p' <<<"$CLAIM_OUT")
WELCOME_OUT=$(pair welcome "$K1" "127.0.0.1:$PORT1")
note "welcome: $WELCOME_OUT"
S2b=$(state_of 1)
NOW_CLAIM=$(pair claim "$K1" "127.0.0.1:$PORT1")
if [ "$S2a" = "invitation open" ] && grep -q '^claim verified' <<<"$CLAIM_OUT" &&
  [ -n "$KEYAPP_FP" ] && [ "$KEYAPP_FP" = "$FP1" ] && [ "$WELCOME_OUT" = welcomed ] &&
  [ "$S2b" = paired ]; then
  pass 2 "the state went unpaired, invitation open, paired; the claim verified; both sides show the words: $FP1"
else
  bad 2 "state before '$S2a', after '$S2b'; key app words '$KEYAPP_FP' vs Box '$FP1'; welcome: $WELCOME_OUT"
fi

# ---- 3 ---------------------------------------------------------------------------

say "3. Devices on the list pass messages; others cannot"
A="127.0.0.1:$PORT1"
M1=$(pair mailbox "$K1" "$A" c1)
M2=$(pair mailbox "$K1" "$A" c2)
MO=$(pair mailbox "$K1" "$A" owner)
SEND1=$(pair send "$K1" "$A" c1 owner "hello-from-c1")
RECV1=$(pair recv "$K1" "$A" owner)
SEND2=$(pair send "$K1" "$A" owner c2 "hello-to-c2")
RECV2=$(pair recv "$K1" "$A" c2)
MBS=$(pair mailbox "$K1" "$A" stranger)
SENDS=$(pair send "$K1" "$A" stranger owner "no")
C2KEY=$(pair key "$K1" c2)
note "$RECV1"
note "$RECV2"
note "stranger: $MBS / $SENDS"
if grep -q '^mailbox made' <<<"$M1" && grep -q '^mailbox made' <<<"$M2" && grep -q '^mailbox made' <<<"$MO" &&
  grep -q 'message from c1: hello-from-c1' <<<"$RECV1" && grep -q 'message from owner: hello-to-c2' <<<"$RECV2" &&
  grep -q '^refused' <<<"$MBS" && grep -q '^refused' <<<"$SENDS" && [ -e "$B1/relay/$C2KEY.mbx" ]; then
  pass 3 "c1 to the owner and the owner to c2 went through the Box and opened; a device not on the list was refused a mailbox and a message"
else
  bad 3 "mailboxes: $M1 / $M2 / $MO; recv: $RECV1 / $RECV2; stranger: $MBS / $SENDS"
fi

# ---- 4 ---------------------------------------------------------------------------

say "4. New settings take a device off the list"
N_BEFORE=$(member_count 1)
SET2=$(pair settings "$K1" "$A" 2 drop-c2)
note "$SET2"
GONE=
for _ in $(seq 1 40); do
  N=$(member_count 1)
  if [ -n "$N" ] && [ "$N" -lt "${N_BEFORE:-0}" ]; then GONE=yes; break; fi
  sleep 0.5
done
# LK-28, LK-29: c2's mailbox is now leaving. It cannot be made again, c2
# can still read what it holds, and a read to its end deletes it.
MB_C2=$(pair mailbox "$K1" "$A" c2)
LEFT_C2=$([ -e "$B1/relay/$C2KEY.mbx" ] && echo yes || echo no)
FETCH_C2=$(pair fetch "$K1" "$A" c2)
AGAIN_C2=$(pair fetch "$K1" "$A" c2)
note "members before $N_BEFORE, after $N"
note "c2: make: $MB_C2 / leaving file: $LEFT_C2 / fetch: $FETCH_C2 / again: $AGAIN_C2"
if [ -n "$GONE" ] && grep -q '^refused' <<<"$MB_C2" && [ "$LEFT_C2" = yes ] &&
  grep -q '^received' <<<"$(tail -1 <<<"$FETCH_C2")" && grep -q '^refused' <<<"$AGAIN_C2" &&
  [ ! -e "$B1/relay/$C2KEY.mbx" ] && grep -q 'mailbox made for c1' <<<"$(pair mailbox "$K1" "$A" c1)"; then
  pass 4 "settings version 2 (c2 off the list) went in as a sealed message; the members fell from $N_BEFORE to $N; c2 could not make a mailbox, read its leaving one once, and then it was gone; c1 still works"
else
  bad 4 "members $N_BEFORE -> ${N:-?}; c2: make: $MB_C2 / leaving file: $LEFT_C2 / fetch: $FETCH_C2 / again: $AGAIN_C2 / file now: $([ -e "$B1/relay/$C2KEY.mbx" ] && echo yes || echo no)"
fi

# ---- 5 ---------------------------------------------------------------------------

say "5. A restart on the same directory"
MEM_BEFORE=$(members_of 1)
stop_box 1
start_box 1 "$B1" "$PORT1" || die "the Box did not start again: $(cat "${LOG[1]}")"
KEY1_AGAIN=$(sed -n 's/^key: //p' "${LOG[1]}")
S5=$(state_of 1)
MEM_AFTER=$(members_of 1)
note "state $S5, key $KEY1_AGAIN"
if [ "$S5" = paired ] && [ "$KEY1_AGAIN" = "$KEY1" ] && [ -n "$MEM_BEFORE" ] && [ "$MEM_BEFORE" = "$MEM_AFTER" ]; then
  pass 5 "after the restart the Box is paired, has the same key and the same $(wc -l <<<"$MEM_AFTER") members"
else
  bad 5 "state '$S5'; key before $KEY1, after $KEY1_AGAIN; members equal: $([ "$MEM_BEFORE" = "$MEM_AFTER" ] && echo yes || echo no)"
fi

# ---- 6 ---------------------------------------------------------------------------

say "6. A second process on the same directory"
SECOND=$(timeout 20 "$BOX_BIN" "$B1" "127.0.0.1:$PORT2" </dev/null 2>&1)
RC=$?
note "exit $RC: $SECOND"
if [ "$RC" != 0 ] && grep -q 'Another OpenBMT program is already using' <<<"$SECOND"; then
  pass 6 "the second process was refused with: $SECOND"
else
  bad 6 "the second process was not refused (exit $RC): $SECOND"
fi

# ---- 7 ---------------------------------------------------------------------------

say "7. Settings that drop the Box"
SET3=$(pair settings "$K1" "$A" 3 drop-box)
note "$SET3"
if await_state 1 "turned off" 20; then OFF=yes; else OFF=; fi
N7=$(member_count 1)
note "members after: $N7"
if [ -n "$OFF" ] && [ "$N7" = 0 ] &&
  grep -q '^refused' <<<"$(pair mailbox "$K1" "$A" c1)"; then
  pass 7 "the state is turned off, the member list is empty and a mailbox is refused"
else
  bad 7 "turned off: ${OFF:-no}; members $N7"
fi
stop_box 1

# ---- 8 ---------------------------------------------------------------------------

say "8. A mistyped invitation and a different invitation"
start_box 2 "$B2" "$PORT2" || die "the second Box did not start: $(cat "${LOG[2]}")"
INV2=$(pair invite "$K2")
FIRST=${INV2:0:1}
if [ "$FIRST" = 0 ]; then OTHER=1; else OTHER=0; fi
TYPO="$OTHER${INV2:1}"
# The refusal is the heading, then what happened on the next line.
ask 2 "invite $TYPO" 'Invitation not read' || true
TYPO_ANSWER=$(grep -A1 '^Invitation not read' "${LOG[2]}" | tail -2 | tr '\n' ' ')
note "typed with one character changed: $TYPO_ANSWER"
S8a=$(state_of 2)
CLAIM8=$(pair claim "$K2" "127.0.0.1:$PORT2")
ask 2 "invite $INV2" 'invitation' || true
S8b=$(state_of 2)
# The Key app makes a different invitation, replacing the one it held.
INV3=$(pair invite "$K2")
CLAIM_WRONG=$(pair claim "$K2" "127.0.0.1:$PORT2")
WELCOME_AFTER=$(pair welcome "$K2" "127.0.0.1:$PORT2")
note "different invitation: $CLAIM_WRONG"
if grep -q 'do not match the rest' <<<"$TYPO_ANSWER" && [ "$S8a" = unpaired ] &&
  grep -q 'no open invitation' <<<"$CLAIM8" && [ "$S8b" = "invitation open" ] &&
  grep -q 'claim REJECTED' <<<"$CLAIM_WRONG" && ! grep -q '^welcomed' <<<"$WELCOME_AFTER"; then
  pass 8 "the typo was refused by its check and left the Box unpaired with no claim; with a valid invitation open, the Key app holding another one rejected the claim and sent no welcome"
else
  bad 8 "typo answer '$TYPO_ANSWER' (state $S8a); claim after typo: $CLAIM8; state with real invitation $S8b; wrong-invitation claim: $CLAIM_WRONG; welcome: $WELCOME_AFTER"
fi
stop_box 2

# ---- 9 ---------------------------------------------------------------------------

say "9. A stranger's welcome while the invitation is open"
A2="127.0.0.1:$PORT2"
start_box 2 "$B2" "$PORT2" || die "the second Box did not start again: $(cat "${LOG[2]}")"
INV9=$(pair invite "$K3")
ask 2 "invite $INV9" 'invitation' || true
S9a=$(state_of 2)
BAD1=$(pair badwelcome "$K3" "$A2")
sleep 2
BAD2=$(pair badwelcome "$K3" "$A2")
sleep 2
S9b=$(state_of 2)
CLAIM9=$(pair claim "$K3" "$A2")
WELCOME9=$(pair welcome "$K3" "$A2")
S9c=$(state_of 2)
note "stranger: $BAD1 / $BAD2"
note "state after: $S9b; claim: $(head -1 <<<"$CLAIM9"); welcome: $WELCOME9"
if [ "$S9a" = "invitation open" ] && grep -q '^refused' <<<"$BAD1" && grep -q '^refused' <<<"$BAD2" &&
  [ "$S9b" = "invitation open" ] && grep -q '^claim verified' <<<"$CLAIM9" &&
  [ "$WELCOME9" = welcomed ] && [ "$S9c" = paired ]; then
  pass 9 "two welcomes from a stranger were refused and left the invitation open; the real claim then verified and the real welcome paired the Box"
else
  bad 9 "states '$S9a' '$S9b' '$S9c'; stranger: $BAD1 / $BAD2; claim: $CLAIM9; welcome: $WELCOME9"
fi
stop_box 2


# ---- 15 --------------------------------------------------------------------------

say "15. A Box without a console, ended by SIGKILL at several points"
B5="$BASE/box5"
K5="$BASE/keyapp5"
mkdir -p "$B5" "$K5"
A5="127.0.0.1:$PORT2"
NC_PID=
NC_LOG="$BASE/box-5.log"
NC_BAD=
# Start the Box with --no-console and no standard input; wait for its state.
start_nc() { # extra arguments
  : >"$NC_LOG"
  "$BOX_BIN" "$B5" "$A5" --no-console "$@" </dev/null >"$NC_LOG" 2>&1 &
  NC_PID=$!
  for _ in $(seq 1 40); do
    grep -q '^state: ' "$NC_LOG" && return 0
    kill -0 "$NC_PID" 2>/dev/null || return 1
    sleep 0.25
  done
  return 1
}
# SIGKILL it at once, as the power going would.
kill9_nc() {
  kill -9 "$NC_PID" 2>/dev/null
  wait "$NC_PID" 2>/dev/null
  NC_PID=
}
# Start again and expect `state` and the same key. A Box that cannot read a
# file stops with its words on the error stream and prints no state.
restart_nc() { # expected state, label
  if ! start_nc; then
    NC_BAD="$NC_BAD; $2: did not start: $(cat "$NC_LOG")"
    return 1
  fi
  local s k
  s=$(sed -n 's/^state: //p' "$NC_LOG")
  k=$(sed -n 's/^key: //p' "$NC_LOG")
  [ -z "${NC_KEY:-}" ] && NC_KEY=$k
  if [ "$s" != "$1" ] || [ "$k" != "$NC_KEY" ]; then
    NC_BAD="$NC_BAD; $2: state '$s' (wanted '$1'), key '$k' (wanted '$NC_KEY'), log: $(cat "$NC_LOG")"
  fi
}
NC_KEY=
INV5=$(pair invite "$K5")
printf '%s\n' "$INV5" >"$BASE/invitation5.txt"
start_nc --invitation-file "$BASE/invitation5.txt" || NC_BAD="$NC_BAD; first start: $(cat "$NC_LOG")"
NC_KEY=$(sed -n 's/^key: //p' "$NC_LOG")
note "first start: $(sed -n 's/^state: //p' "$NC_LOG"); invitation file left: $([ -e "$BASE/invitation5.txt" ] && echo yes || echo no)"
# (a) right after the invitation was stored.
kill9_nc
restart_nc "invitation open" "after the invitation"
CLAIM5=$(pair claim "$K5" "$A5")
WELCOME5=$(pair welcome "$K5" "$A5")
# (b) right after the welcome was accepted.
kill9_nc
restart_nc paired "after the welcome"
M5=$(pair mailbox "$K5" "$A5" c1; pair mailbox "$K5" "$A5" owner)
# (c) right after the mailboxes were made.
kill9_nc
restart_nc paired "after the mailboxes"
SEND5=$(pair send "$K5" "$A5" c1 owner "survives-a-kill")
# (d) right after a message was accepted.
kill9_nc
restart_nc paired "after the message"
RECV5=$(pair recv "$K5" "$A5" owner)
# (e) right after new settings were sent (applied or not yet, never torn).
SET5=$(pair settings "$K5" "$A5" 2 drop-c2)
kill9_nc
restart_nc paired "after the settings"
# (f) and once more, with a kill straight after a start.
kill9_nc
restart_nc paired "after a second kill"
# A stop by SIGTERM, the default action, as `docker stop` gives it.
kill -TERM "$NC_PID" 2>/dev/null
wait "$NC_PID" 2>/dev/null
RC15=$?
NC_PID=
restart_nc paired "after SIGTERM"
note "claim: $CLAIM5 / welcome: $WELCOME5 / mailboxes: $(tr '\n' ' ' <<<"$M5") / send: $SEND5 / recv: $RECV5 / settings: $SET5"
if [ -z "$NC_BAD" ] && grep -q '^claim verified' <<<"$CLAIM5" && [ "$WELCOME5" = welcomed ] &&
  grep -q 'message from c1: survives-a-kill' <<<"$RECV5" && [ "$RC15" = 143 ]; then
  pass 15 "a Box with no console and no standard input, killed with SIGKILL after the invitation, the welcome, the mailboxes, a message and new settings, and once more, started again every time with the same key and the right state, and the message sent before a kill was read after it; SIGTERM ended it with exit 143"
else
  bad 15 "exit after SIGTERM $RC15; claim: $CLAIM5; welcome: $WELCOME5; recv: $RECV5; problems: ${NC_BAD:-none}"
fi
if [ -n "$NC_PID" ]; then kill9_nc; fi

# ---- 17 --------------------------------------------------------------------------

say "17. The status page: its words and headers, a setup code, a wrong invitation, a foreign origin"
B7="$BASE/box7"
K7="$BASE/keyapp7"
mkdir -p "$B7" "$K7"
A7="127.0.0.1:$PORT2"
PAGE7="127.0.0.1:$PORT3"
PG_LOG="$BASE/box-7.log"
PG_PID=
PG_BAD=
: >"$PG_LOG"
"$BOX_BIN" "$B7" "$A7" --no-console --status-page "$PAGE7" </dev/null >"$PG_LOG" 2>&1 &
PG_PID=$!
for _ in $(seq 1 40); do
  grep -q '^state: ' "$PG_LOG" && break
  kill -0 "$PG_PID" 2>/dev/null || break
  sleep 0.25
done
grep -q "^status page: $PAGE7\$" "$PG_LOG" || PG_BAD="$PG_BAD; the Box did not say its page address: $(cat "$PG_LOG")"
# The setup code is printed right after the page's address (BX-62).
CODE7=$(sed -n 's/^setup code: //p' "$PG_LOG" | head -1)
grep -A1 '^status page: ' "$PG_LOG" | grep -q '^setup code: [0-9A-HJKMNP-TV-Z]\{4\}-[0-9A-HJKMNP-TV-Z]\{4\}$' ||
  PG_BAD="$PG_BAD; the Box did not print its setup code after the page address: $(cat "$PG_LOG")"
# The page, with its headers.
PG_GET=$(curl -s --max-time 10 -D "$BASE/pg-head" -o "$BASE/pg-body" -w '%{http_code}' "http://$PAGE7/")
[ "$PG_GET" = 200 ] || PG_BAD="$PG_BAD; GET / gave $PG_GET"
for h in "Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; frame-ancestors 'none'" \
  'X-Content-Type-Options: nosniff' 'Referrer-Policy: no-referrer' 'Cache-Control: no-store' 'Connection: close'; do
  grep -qiF -- "$h" "$BASE/pg-head" || PG_BAD="$PG_BAD; header missing: $h"
done
grep -q 'Waiting for an invitation' "$BASE/pg-body" || PG_BAD="$PG_BAD; the page does not say the Box is waiting"
grep -q 'Next:' "$BASE/pg-body" || PG_BAD="$PG_BAD; the page does not say what to do next"
grep -q "127.0.0.1:${A7##*:}" "$BASE/pg-body" || PG_BAD="$PG_BAD; the page does not give the relay address"
if grep -qiE '<script|src=|https?://' "$BASE/pg-body"; then PG_BAD="$PG_BAD; the page is not self-contained"; fi
grep -q 'name="setup"' "$BASE/pg-body" || PG_BAD="$PG_BAD; the page has no field for the setup code"
if grep -qF -- "$CODE7" "$BASE/pg-body"; then PG_BAD="$PG_BAD; the setup code is on the page"; fi
# Another path and another method are refused with the status alone.
PG_404=$(curl -s --max-time 10 -o /dev/null -w '%{http_code}' "http://$PAGE7/nothing")
PG_405=$(curl -s --max-time 10 -o /dev/null -w '%{http_code}' -X DELETE "http://$PAGE7/")
[ "$PG_404" = 404 ] && [ "$PG_405" = 405 ] || PG_BAD="$PG_BAD; other path $PG_404, other method $PG_405"
# A right invitation with a wrong setup code, from the page's own origin:
# refused with words, and the page then refuses everything for ten seconds.
INV7=$(pair invite "$K7")
PG_NOCODE=$(curl -s --max-time 10 -o "$BASE/pg-nocode" -w '%{http_code}' -H "Origin: http://$PAGE7" \
  --data-urlencode "setup=0000-0000" --data-urlencode "invitation=$INV7" "http://$PAGE7/invitation")
PG_HELD=$(curl -s --max-time 10 -o /dev/null -w '%{http_code}' -H "Origin: http://$PAGE7" \
  --data-urlencode "setup=$CODE7" --data-urlencode "invitation=$INV7" "http://$PAGE7/invitation")
[ "$PG_NOCODE" = 403 ] && [ "$PG_HELD" = 403 ] && grep -q 'Setup code wrong' "$BASE/pg-nocode" ||
  PG_BAD="$PG_BAD; wrong setup code $PG_NOCODE, right code inside the ten seconds $PG_HELD"
if grep -qF -- "$CODE7" "$BASE/pg-nocode"; then PG_BAD="$PG_BAD; the setup code is on a refusal"; fi
curl -s --max-time 10 "http://$PAGE7/" | grep -q 'Waiting for an invitation' || PG_BAD="$PG_BAD; a refused code changed the state"
sleep 11
# A wrong invitation, from the page's own origin: refused with words.
WRONG7="${INV7%?}$([ "${INV7: -1}" = 0 ] && echo 1 || echo 0)"
PG_WRONG=$(curl -s --max-time 10 -o "$BASE/pg-wrong" -w '%{http_code}' -H "Origin: http://$PAGE7" \
  --data-urlencode "setup=$CODE7" --data-urlencode "invitation=$WRONG7" "http://$PAGE7/invitation")
[ "$PG_WRONG" = 422 ] && grep -q 'Invitation not read' "$BASE/pg-wrong" || PG_BAD="$PG_BAD; wrong invitation: status $PG_WRONG"
grep -q 'Waiting for an invitation' "$BASE/pg-wrong" || PG_BAD="$PG_BAD; the Box is not waiting after a wrong invitation"
# A right invitation from a foreign origin, and with no origin at all: refused.
PG_FOREIGN=$(curl -s --max-time 10 -o "$BASE/pg-foreign" -w '%{http_code}' -H 'Origin: http://evil.example' \
  --data-urlencode "invitation=$INV7" "http://$PAGE7/invitation")
PG_NONE=$(curl -s --max-time 10 -o /dev/null -w '%{http_code}' --data-urlencode "invitation=$INV7" "http://$PAGE7/invitation")
[ "$PG_FOREIGN" = 403 ] && [ "$PG_NONE" = 403 ] && grep -q 'Not sent from this page' "$BASE/pg-foreign" ||
  PG_BAD="$PG_BAD; foreign origin $PG_FOREIGN, no origin $PG_NONE"
curl -s --max-time 10 "http://$PAGE7/" | grep -q 'Waiting for an invitation' || PG_BAD="$PG_BAD; a refused post changed the state"
# The right invitation from the page's own origin is taken; the Key app side
# then claims and welcomes, and the page says paired.
PG_RIGHT=$(curl -s --max-time 10 -o "$BASE/pg-right" -w '%{http_code}' -H "Origin: http://$PAGE7" \
  --data-urlencode "setup=$CODE7" --data-urlencode "invitation=$INV7" "http://$PAGE7/invitation")
[ "$PG_RIGHT" = 200 ] && grep -q 'Invitation open' "$BASE/pg-right" || PG_BAD="$PG_BAD; right invitation: status $PG_RIGHT"
CLAIM7=$(pair claim "$K7" "$A7")
WELCOME7=$(pair welcome "$K7" "$A7")
grep -q '^claim verified' <<<"$CLAIM7" && [ "$WELCOME7" = welcomed ] || PG_BAD="$PG_BAD; claim: $CLAIM7; welcome: $WELCOME7"
curl -s --max-time 10 "http://$PAGE7/" >"$BASE/pg-paired"
grep -q '<h2>Paired</h2>' "$BASE/pg-paired" || PG_BAD="$PG_BAD; the page does not say paired"
if grep -qF -- "$INV7" "$BASE/pg-paired" "$BASE/pg-right"; then PG_BAD="$PG_BAD; the invitation is on a page"; fi
note "GET $PG_GET; other path $PG_404; other method $PG_405; wrong setup code $PG_NOCODE; right code inside the hold $PG_HELD; wrong invitation $PG_WRONG; foreign origin $PG_FOREIGN; no origin $PG_NONE; right invitation $PG_RIGHT"
if [ -z "$PG_BAD" ]; then
  pass 17 "a Box started with --status-page answered GET / with the security headers and words that say it is waiting for an invitation and what to do next; another path was 404 and another method 405; the setup code was printed right after the page's address and never on a page, a wrong code was refused with words (403) and made the page refuse the right one for ten seconds; a wrong invitation with the right code was refused with words (422), a right one from a foreign origin or with no origin was refused (403) and changed nothing; the right one from the page's own origin was taken, the Key app side claimed and welcomed it, and the page then said paired with no invitation on it"
else
  bad 17 "${PG_BAD#; }"
fi
kill -9 "$PG_PID" 2>/dev/null
wait "$PG_PID" 2>/dev/null
PG_PID=

# ---- 18 --------------------------------------------------------------------------

say "18. The phones' libraries pair a real Box"
[ -z "${PID[2]:-}" ] || stop_box 2 || true
B8="$BASE/box8"
mkdir -p "$B8"
PH_BAD=
start_box 2 "$B8" "$PORT2" || die "the Box did not start: $(cat "${LOG[2]}")"
FP8=$(sed -n 's/^fingerprint: //p' "${LOG[2]}")
KEY8=$(sed -n 's/^key: //p' "${LOG[2]}")
S8a=$(state_of 2)
PH_LOG="$BASE/phones.log"
# The Key app (openbmt-key-ffi) makes the invitation and types it into the
# Box's console; the owner phone (openbmt-owner-ffi) fetches the claim, the
# Key app checks it and welcomes the Box in an approval session, the owner
# phone hands the Box the welcome, and the owner's link passes a sealed
# message through it. All of that is the ignored test named below.
phones_test() {
  local guard=$HOME/.cache/openbmt/guard.sh run=(env CARGO_TARGET_DIR="$TARGET" CARGO_BUILD_JOBS=2
    OPENBMT_BOX_E2E_ADDR="127.0.0.1:$PORT2" OPENBMT_BOX_E2E_CONSOLE="$BASE/stdin-2"
    cargo test --quiet --locked --manifest-path "$ROOT/Cargo.toml" -p openbmt-owner-ffi --lib
    oa_41_to_oa_43_a_real_box_process_is_paired -- --ignored --nocapture)
  if [ -x "$guard" ]; then "$guard" "${run[@]}"; else "${run[@]}"; fi
}
if phones_test >"$PH_LOG" 2>&1; then PH_RC=0; else PH_RC=$?; fi
note "$(grep '^phones:' "$PH_LOG" | sed 's/^/phones side: /')"
S8b=$(state_of 2)
PH_WORDS=$(sed -n 's/^phones: box words: //p' "$PH_LOG")
PH_KEY=$(sed -n 's/^phones: box key: //p' "$PH_LOG")
PH_MSG=$(sed -n 's/^phones: message: //p' "$PH_LOG")
[ "$PH_RC" = 0 ] || PH_BAD="$PH_BAD; the phones' test failed (exit $PH_RC): $(tail -15 "$PH_LOG")"
[ "$S8a" = unpaired ] || PH_BAD="$PH_BAD; the Box was '$S8a' before"
[ "$S8b" = paired ] || PH_BAD="$PH_BAD; the Box is '$S8b' after"
[ -n "$PH_WORDS" ] && [ "$PH_WORDS" = "$FP8" ] || PH_BAD="$PH_BAD; the phones showed '$PH_WORDS' and the Box '$FP8'"
[ -n "$PH_KEY" ] && [ "$PH_KEY" = "$KEY8" ] || PH_BAD="$PH_BAD; the phones' Box key '$PH_KEY' is not the Box's '$KEY8'"
[ "$PH_MSG" = "hello through the Box" ] || PH_BAD="$PH_BAD; the sealed message read '$PH_MSG'"
grep -q '^phones: paired' "$PH_LOG" || PH_BAD="$PH_BAD; the phones did not say paired"
if [ -z "$PH_BAD" ]; then
  pass 18 "the Key app library made the invitation and the Box took it; the owner library fetched the claim, the Key app library checked it and welcomed the Box in an approval session, the owner library delivered it, and the Box went from unpaired to paired; both phones showed the Box's words ($FP8); the owner's link then passed a sealed message through it"
else
  bad 18 "${PH_BAD#; }"
fi
stop_box 2 || true

# ---- result ----------------------------------------------------------------------

say "Result"
failed=0
notrun=0
for n in 1 2 3 4 5 6 7 8 9 15 17 18; do
  printf '%s  step %s\n' "${RESULT[$n]:-FAIL}" "$n"
  case "${RESULT[$n]:-FAIL}" in
    PASS) ;;
    "NOT RUN") notrun=$((notrun + 1)) ;;
    *) failed=1 ;;
  esac
done
if [ "$failed" != 0 ]; then
  echo "SOME STEPS FAILED"
  exit 1
elif [ "$notrun" != 0 ]; then
  echo "PASS for the steps that ran; $notrun steps NOT RUN"
else
  echo "ALL PASS"
fi
