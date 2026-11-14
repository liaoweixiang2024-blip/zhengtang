-- 通知渠道启停（临时静默某个渠道不删配置）
ALTER TABLE notify_channels ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1;
