-- 购买与流量账期
ALTER TABLE servers ADD COLUMN purchased_at INTEGER;
ALTER TABLE servers ADD COLUMN traffic_reset_day INTEGER;  -- 1-28；NULL=自然月
