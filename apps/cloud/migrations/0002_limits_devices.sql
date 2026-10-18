-- 按账号的限制与设备管理（docs/sync-design.md v0.4）
-- users.limits：运营者为单个账号设置的覆盖值（JSON 对象，可含 quota_bytes、max_devices、
-- check_interval_secs、panel_check_secs、min_push_interval_secs）；为空时使用服务端配置的默认值。
ALTER TABLE users ADD COLUMN limits TEXT;

-- 设备授权：桌面端登录与刷新令牌时上报的设备名称、平台与应用版本（明文元数据）；
-- limited = 1 表示该设备超出账号的设备数量限制，在腾出名额之前不能访问保险库。
ALTER TABLE device_grants ADD COLUMN device_name TEXT;
ALTER TABLE device_grants ADD COLUMN platform TEXT;
ALTER TABLE device_grants ADD COLUMN app_version TEXT;
ALTER TABLE device_grants ADD COLUMN limited INTEGER NOT NULL DEFAULT 0;

-- 加入请求的发起设备：版本检查返回"其他设备发起的待处理加入请求"数量时排除本机发起的请求
ALTER TABLE join_requests ADD COLUMN device_id TEXT;
