#!/usr/bin/env bash
# 创建本机开发用自签名代码签名证书 "Qantrip Dev"（见 docs/decisions.md D8）。
# 只用于本地开发，使重新构建后的应用保持同一签名身份，辅助功能授权不失效。
# 存放在独立钥匙串中（固定密码、脚本自行解锁），不改动登录钥匙串。
set -euo pipefail
NAME="Qantrip Dev"
KEYCHAIN="$HOME/Library/Keychains/qantrip-dev.keychain-db"
KC_PASS="qantrip-dev"

[ -f "$KEYCHAIN" ] || security create-keychain -p "$KC_PASS" "$KEYCHAIN"
security unlock-keychain -p "$KC_PASS" "$KEYCHAIN"
security set-keychain-settings "$KEYCHAIN"  # 不自动锁定

if security find-certificate -c "$NAME" "$KEYCHAIN" >/dev/null 2>&1; then
  echo "证书 \"$NAME\" 已存在"; exit 0
fi

TMP=$(mktemp -d); trap 'rm -rf "$TMP"' EXIT
cat > "$TMP/cfg" <<CFG
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = $NAME
[ext]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
CFG
/usr/bin/openssl req -x509 -newkey rsa:2048 -nodes -days 3650 \
  -keyout "$TMP/key.pem" -out "$TMP/cert.pem" -config "$TMP/cfg" 2>/dev/null
/usr/bin/openssl pkcs12 -export -inkey "$TMP/key.pem" -in "$TMP/cert.pem" \
  -name "$NAME" -passout pass:qantrip -out "$TMP/id.p12"
security import "$TMP/id.p12" -k "$KEYCHAIN" -P qantrip -T /usr/bin/codesign
security set-key-partition-list -S apple-tool:,apple: -s -k "$KC_PASS" "$KEYCHAIN" >/dev/null
echo "已导入 \"$NAME\""
