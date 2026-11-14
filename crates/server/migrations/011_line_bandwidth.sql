-- 线路与带宽（VPS 常用属性：CN2 GIA / 500M 这类标注）
ALTER TABLE servers ADD COLUMN line TEXT;
ALTER TABLE servers ADD COLUMN bandwidth_mbps INTEGER;
