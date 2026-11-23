#!/bin/sh
# Writes the test mailboxes into the maildata volume (/srv/vmail). Runs once in an Alpine
# container (compose service "seed"); the volume is kept until `mail-env.sh reset`.
#
# Messages are put into the maildirs by hand so that their dates and flags are exactly what the
# tests expect: Dovecot takes a message's received date from the file's modification time, and
# a message in new/ is unseen and \Recent. Unseen mail must stay unseen after a Shale sync
# (Shale only ever uses EXAMINE and BODY.PEEK), and the mail-env test checks that.
set -eu

ROOT=/srv/vmail
[ -f "$ROOT/.seeded" ] && { echo "already seeded"; exit 0; }
NOW=$(date +%s)
N=0

# put <user> <folder> <new|cur> <flags> <age-in-days> <subject-header-line> <from> [content-type-header]
#   the body comes from standard input, the whole message is built here
put() {
  user=$1 folder=$2 state=$3 flags=$4 age=$5 subject=$6 from=$7 ctype=${8:-"Content-Type: text/plain; charset=utf-8"}
  case "$folder" in INBOX) dir="$ROOT/$user/mail" ;; *) dir="$ROOT/$user/mail/$folder" ;; esac
  mkdir -p "$dir/cur" "$dir/new" "$dir/tmp"
  N=$((N + 1))
  ts=$((NOW - age * 86400))
  tmp="$dir/tmp/$N.$$"
  {
    printf 'Return-Path: <%s>\r\n' "$(printf '%s' "$from" | sed 's/.*<\(.*\)>.*/\1/')"
    printf 'From: %s\r\n' "$from"
    printf 'To: %s\r\n' "$user"
    printf '%s\r\n' "$subject"
    printf 'Date: %s\r\n' "$(date -u -d "@$ts" '+%a, %d %b %Y %H:%M:%S +0000')"
    printf 'Message-ID: <seed-%s-%s@shale.test>\r\n' "$N" "$ts"
    printf 'MIME-Version: 1.0\r\n'
    printf '%s\r\n' "$ctype"
    printf '\r\n'
    cat
  } > "$tmp"
  size=$(wc -c < "$tmp" | tr -d ' ')
  if [ "$state" = new ]; then
    name="$ts.M${N}P$$.seed,S=$size"
    mv "$tmp" "$dir/new/$name"
    touch -d "@$ts" "$dir/new/$name"
  else
    name="$ts.M${N}P$$.seed,S=$size:2,$flags"
    mv "$tmp" "$dir/cur/$name"
    touch -d "@$ts" "$dir/cur/$name"
  fi
}

A=alice@shale.test
O=oauth@shale.test
BOB='Bob Builder <bob@example.org>'

# ---- alice (password) -----------------------------------------------------------------
echo "Please pay the invoice for the lecture hall by Friday." | put $A INBOX new "" 4 "Subject: Invoice 2026" "$BOB"
echo "Pizza at noon, projector later." | put $A INBOX new "" 3 "Subject: Lunch plans" "Dora <dora@example.org>"
echo "Agenda: budget, hiring, the new office." | put $A INBOX cur FS 10 "Subject: Meeting notes" "Carol <carol@example.org>"
echo "This one is two years old and inside the three-year window." | put $A INBOX cur S 730 "Subject: Two years ago" "$BOB"
echo "This one is older than three years and must not be synced." | put $A INBOX cur S 1200 "Subject: Ancient history" "$BOB"
# a large attachment (about 6 MB as base64)
{
  printf -- '--SEEDB\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nThe big file is attached.\r\n'
  printf -- '--SEEDB\r\nContent-Type: application/octet-stream; name="big.bin"\r\nContent-Disposition: attachment; filename="big.bin"\r\nContent-Transfer-Encoding: base64\r\n\r\n'
  head -c 4500000 /dev/urandom | base64
  printf -- '\r\n--SEEDB--\r\n'
} | put $A INBOX cur S 5 "Subject: Big attachment" "Carol <carol@example.org>" 'Content-Type: multipart/mixed; boundary="SEEDB"'
# a small WAV attachment (1644 bytes: 0.2 s of a 440 Hz tone, 8 kHz, 8 bit mono), the reading pane offers the player for it (MAIL-242)
{
  printf -- '--SEEDW\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nA voice memo is attached.\r\n'
  printf -- '--SEEDW\r\nContent-Type: audio/wav; name="voice.wav"\r\nContent-Disposition: attachment; filename="voice.wav"\r\nContent-Transfer-Encoding: base64\r\n\r\n'
  cat <<'WAVDATA'
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
WAVDATA
  printf -- '\r\n--SEEDW--\r\n'
} | put $A INBOX cur S 4 "Subject: Voice memo" "Carol <carol@example.org>" 'Content-Type: multipart/mixed; boundary="SEEDW"'
# HTML only, no text part
printf '<html><body><h1>Spring news</h1><p>Hello <b>reader</b>, <a href="https://example.org/offer">see the offer</a>.</p><img src="https://tracker.example.org/pixel.gif"></body></html>\r\n' \
  | put $A INBOX new "" 2 "Subject: HTML only newsletter" "News <news@example.org>" "Content-Type: text/html; charset=utf-8"
# 8-bit, non-UTF-8 headers and body (ISO-8859-1): "Gr\374\337e aus K\366ln"
printf 'Gr\374\337e aus K\366ln, sch\366ne Gr\374\337e!\r\n' \
  | put $A INBOX cur S 6 "$(printf 'Subject: Gr\374\337e aus K\366ln')" "$(printf 'J\374rgen M\374ller <juergen@example.org>')" "Content-Type: text/plain; charset=iso-8859-1"
echo "A work item in a folder." | put $A Work cur S 20 "Subject: Work item" "Carol <carol@example.org>"
echo "The plan for the project, in a nested folder." | put $A Work/Projects new "" 15 "Subject: Project plan" "Carol <carol@example.org>"
echo "An old project mail, older than three years." | put $A Work/Projects cur S 1300 "Subject: Nested old" "Carol <carol@example.org>"
echo "Paid, thanks." | put $A Sent cur S 3 "Subject: Re: Invoice 2026" "$A"
echo "Unsubscribe me." | put $A Trash cur S 40 "Subject: Old newsletter" "News <news@example.org>"
echo "Buy now." | put $A Trash cur S 41 "Subject: Spam offer" "Spam <spam@example.org>"
echo "You won a prize." | put $A Junk cur "" 5 "Subject: Win a prize" "Prize <prize@example.org>"
mkdir -p "$ROOT/$A/mail/Drafts/cur" "$ROOT/$A/mail/Drafts/new" "$ROOT/$A/mail/Drafts/tmp"

# ---- oauth (token) --------------------------------------------------------------------
echo "You signed in with OAuth2 and read this." | put $O INBOX new "" 1 "Subject: Hello from OAuth land" "Ada <ada@example.org>"
echo "A second mail, already read." | put $O INBOX cur S 7 "Subject: Second mail" "Ada <ada@example.org>"
echo "Sent with OAuth." | put $O Sent cur S 2 "Subject: Sent via OAuth" "$O"
echo "Deleted." | put $O Trash cur S 30 "Subject: Deleted thing" "Ada <ada@example.org>"

chown -R 1000:1000 "$ROOT"
touch "$ROOT/.seeded"
echo "seeded: $(find "$ROOT" -type f -path '*/cur/*' -o -type f -path '*/new/*' | wc -l) messages"
