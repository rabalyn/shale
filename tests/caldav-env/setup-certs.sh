#!/bin/sh
# Makes a throw-away certificate authority and a server certificate for localhost into ./certs
# (git-ignored). Run by caldav-env.sh up; safe to run again (it does nothing if the files exist).
# The CA is only for this test environment: never trust it for anything else, and delete the
# folder when you are done.
set -eu
cd "$(dirname "$0")"
mkdir -p certs
if [ -f certs/ca.crt ] && [ -f certs/tls.crt ] && [ -f certs/tls.key ]; then
  exit 0
fi
umask 077
openssl req -x509 -newkey rsa:3072 -nodes -days 365 -subj "/CN=Shale CalDAV test CA" \
  -addext "basicConstraints=critical,CA:TRUE,pathlen:0" -addext "keyUsage=critical,keyCertSign,cRLSign" \
  -keyout certs/ca.key -out certs/ca.crt
openssl req -newkey rsa:3072 -nodes -subj "/CN=localhost" -keyout certs/tls.key -out certs/tls.csr
printf 'subjectAltName=DNS:localhost,IP:127.0.0.1\nbasicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n' > certs/ext.cnf
openssl x509 -req -in certs/tls.csr -CA certs/ca.crt -CAkey certs/ca.key -CAcreateserial -days 365 -extfile certs/ext.cnf -out certs/tls.crt
rm -f certs/tls.csr certs/ext.cnf certs/ca.srl
# the Dovecot container reads these as another user
chmod 755 certs
chmod 644 certs/ca.crt certs/tls.crt certs/tls.key
echo "certificates written to $(pwd)/certs (CA: certs/ca.crt)"
