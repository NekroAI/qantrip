-- Nekro Qantrip 云服务初始结构（docs/sync-design.md v0.2，docs/decisions.md D23 / D24）
-- 服务端只保存密文与最少元数据；所有查询按主键或复合索引访问（D1 按扫描行数计费）。

-- 账号（GitHub 登录）
CREATE TABLE users (
  id            TEXT PRIMARY KEY,
  github_id     INTEGER NOT NULL UNIQUE,
  login         TEXT NOT NULL,
  avatar_url    TEXT,
  created_at    INTEGER NOT NULL,
  last_login_at INTEGER NOT NULL
);

-- 官网会话：只存令牌哈希
CREATE TABLE web_sessions (
  token_hash TEXT PRIMARY KEY,
  user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  created_at INTEGER NOT NULL,
  expires_at INTEGER NOT NULL
);
CREATE INDEX web_sessions_user ON web_sessions(user_id);
CREATE INDEX web_sessions_expires ON web_sessions(expires_at);

-- 网页登录的 state（防 CSRF），短期有效
CREATE TABLE login_states (
  state      TEXT PRIMARY KEY,
  return_to  TEXT NOT NULL,
  expires_at INTEGER NOT NULL
);
CREATE INDEX login_states_expires ON login_states(expires_at);

-- 桌面端授权码（PKCE S256），只存哈希
CREATE TABLE oauth_codes (
  code_hash      TEXT PRIMARY KEY,
  user_id        TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  device_id      TEXT NOT NULL,
  code_challenge TEXT NOT NULL,
  redirect_uri   TEXT NOT NULL,
  expires_at     INTEGER NOT NULL
);
CREATE INDEX oauth_codes_expires ON oauth_codes(expires_at);

-- 设备访问授权：每台设备一条，访问令牌与刷新令牌只存哈希
CREATE TABLE device_grants (
  id                 TEXT PRIMARY KEY,
  user_id            TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  device_id          TEXT NOT NULL,
  access_hash        TEXT NOT NULL UNIQUE,
  access_expires_at  INTEGER NOT NULL,
  refresh_hash       TEXT NOT NULL UNIQUE,
  refresh_expires_at INTEGER NOT NULL,
  created_at         INTEGER NOT NULL,
  last_used_at       INTEGER NOT NULL,
  revoked_at         INTEGER
);
CREATE INDEX device_grants_user ON device_grants(user_id);
CREATE INDEX device_grants_device ON device_grants(user_id, device_id);

-- 保险库：每个账号一个
CREATE TABLE vaults (
  id           TEXT PRIMARY KEY,
  user_id      TEXT NOT NULL UNIQUE REFERENCES users(id) ON DELETE CASCADE,
  seq          INTEGER NOT NULL DEFAULT 0,
  roster_len   INTEGER NOT NULL DEFAULT 0,
  min_epoch    INTEGER NOT NULL DEFAULT 1,
  record_count INTEGER NOT NULL DEFAULT 0,
  byte_count   INTEGER NOT NULL DEFAULT 0,
  -- 已清理墓碑的最大 seq：base_seq 不大于它且记录已不存在时返回 gone
  purged_seq   INTEGER NOT NULL DEFAULT 0,
  created_at   INTEGER NOT NULL,
  updated_at   INTEGER NOT NULL
);

-- 签名花名册（只追加；服务端不验证签名，只保证 CAS 与顺序）
CREATE TABLE roster_entries (
  vault_id   TEXT NOT NULL REFERENCES vaults(id) ON DELETE CASCADE,
  idx        INTEGER NOT NULL,
  entry      BLOB NOT NULL,
  created_at INTEGER NOT NULL,
  PRIMARY KEY (vault_id, idx)
);

-- 密文记录
CREATE TABLE records (
  vault_id         TEXT NOT NULL REFERENCES vaults(id) ON DELETE CASCADE,
  record_id        TEXT NOT NULL,
  seq              INTEGER NOT NULL,
  epoch            INTEGER NOT NULL,
  ciphertext       BLOB NOT NULL,
  size             INTEGER NOT NULL,
  deleted          INTEGER NOT NULL DEFAULT 0,
  writer_device_id TEXT NOT NULL,
  updated_at       INTEGER NOT NULL,
  PRIMARY KEY (vault_id, record_id)
);
CREATE INDEX records_seq ON records(vault_id, seq);
CREATE INDEX records_tombstones ON records(deleted, updated_at) WHERE deleted = 1;

-- 信封：HPKE Auth 包装的 VK 钥匙环；recipient 为 device_id 或 'recovery'
CREATE TABLE envelopes (
  vault_id     TEXT NOT NULL REFERENCES vaults(id) ON DELETE CASCADE,
  recipient    TEXT NOT NULL,
  epoch        INTEGER NOT NULL,
  roster_index INTEGER NOT NULL,
  data         BLOB NOT NULL,
  created_at   INTEGER NOT NULL,
  PRIMARY KEY (vault_id, recipient, epoch)
);

-- 新设备加入：服务端只转发四段消息（docs/sync-design.md 6.2）
CREATE TABLE join_requests (
  id         TEXT PRIMARY KEY,
  vault_id   TEXT NOT NULL REFERENCES vaults(id) ON DELETE CASCADE,
  commit_msg BLOB NOT NULL,
  reply_msg  BLOB,
  reveal_msg BLOB,
  result_msg BLOB,
  created_at INTEGER NOT NULL,
  expires_at INTEGER NOT NULL
);
CREATE INDEX join_requests_vault ON join_requests(vault_id);
CREATE INDEX join_requests_expires ON join_requests(expires_at);

-- 推送幂等：保留 24 小时
CREATE TABLE idempotency_keys (
  vault_id   TEXT NOT NULL,
  key        TEXT NOT NULL,
  result     TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  PRIMARY KEY (vault_id, key)
);
CREATE INDEX idempotency_created ON idempotency_keys(created_at);

-- 新建保险库按 IP 计数（每天）
CREATE TABLE vault_creations (
  ip_hash TEXT NOT NULL,
  day     INTEGER NOT NULL,
  count   INTEGER NOT NULL,
  PRIMARY KEY (ip_hash, day)
);

-- 审计：账号级操作，不含任何内容
CREATE TABLE audit_log (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id    TEXT,
  action     TEXT NOT NULL,
  detail     TEXT,
  created_at INTEGER NOT NULL
);
CREATE INDEX audit_user ON audit_log(user_id, created_at);
