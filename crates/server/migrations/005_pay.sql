-- 付费周期与费用
ALTER TABLE servers ADD COLUMN pay_cycle_months INTEGER;   -- 1/3/12/36/60；NULL=未填
ALTER TABLE servers ADD COLUMN price_per_cycle REAL;        -- 每期费用
ALTER TABLE servers ADD COLUMN currency TEXT;               -- CNY/USD/EUR/JPY/HKD/GBP
